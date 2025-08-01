use anyhow::{bail, Context, Result};
use clap::{ArgAction, Parser};
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind};
use ssh2::{Session, Sftp};
use std::collections::{HashMap, VecDeque, HashSet};
use std::fs::{self, File};
use std::net::TcpStream;
use std::os::unix::fs::{MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};
use std::env;

/// Simple, fast SFTP directory mirror: local -> remote
#[derive(Parser, Debug)]
#[command(name = "rmote", author, version, about)]
struct Cli {
    /// Remote host (IP or DNS)
    #[arg(long, env = "RMOTE_HOST")]
    host: String,

    /// Remote SSH port
    #[arg(long, env = "RMOTE_PORT", default_value = "22")]
    port: u16,

    /// SSH username
    #[arg(long, env = "RMOTE_USER", default_value = "root")]
    user: String,

    /// Path to private key (e.g. ~/.ssh/id_ed25519)
    #[arg(long, env = "RMOTE_KEY", default_value = "~/.ssh/id_ed25519")]
    identity: String,

    /// Path to public key (e.g. ~/.ssh/id_ed25519.pub)
    #[arg(long, env = "RMOTE_PUB", default_value = "~/.ssh/id_ed25519.pub")]
    identity_pub: String,

    /// Optional passphrase for the private key
    #[arg(long, env = "RMOTE_PASSPHRASE")]
    passphrase: Option<String>,

    /// Remote base directory to mirror into (created if needed)
    #[arg(long, env = "RMOTE_REMOTE_DIR", default_value = ".")]
    remote_dir: String,

    /// Perform a full sync at startup
    #[arg(long, action = ArgAction::SetTrue, default_value_t = true)]
    initial_sync: bool,

    /// Disable full sync at startup
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "initial_sync")]
    no_initial_sync: bool,

    /// One or more blacklist entries. May be repeated.
    /// Matches if a path equals an entry or starts with it.
    #[arg(long = "blacklist", short = 'x', action = ArgAction::Append)]
    blacklist: Vec<String>,

    /// Debounce window (seconds) to coalesce events
    #[arg(long, default_value_t = 1)]
    debounce_s: u64,
}

#[derive(PartialEq, Debug, Copy, Clone)]
enum Action {
    Transfer,
    Delete,
    None,
}

struct App {
    // sess: Session,
    sftp: Sftp,
    local_root: PathBuf,
    remote_root: PathBuf,
    blacklist: Vec<PathBuf>,
    blacklist_names: HashSet<String>,
    debounce: Duration,
}

impl App {
    fn connect(cli: &Cli) -> Result<Session> {
        let tcp = TcpStream::connect((cli.host.as_str(), cli.port))
            .with_context(|| format!("Connecting to {}:{}", cli.host, cli.port))?;

        let mut sess = Session::new().expect("Failed to create SSH session");
        sess.set_tcp_stream(tcp);
        sess.handshake().context("SSH handshake failed")?;

        let privkey = expand_tilde(&cli.identity);
        let pubkey = expand_tilde(&cli.identity_pub);

        sess.userauth_pubkey_file(
            &cli.user,
            Some(Path::new(&pubkey)),
            Path::new(&privkey),
            cli.passphrase.as_deref(),
        )
        .with_context(|| "SSH public key authentication failed")?;

        if !sess.authenticated() {
            bail!("Authentication failed");
        }
        Ok(sess)
    }

    fn new(cli: &Cli) -> Result<Self> {
        let sess = Self::connect(cli)?;
        let sftp = sess.sftp().context("Opening SFTP subsystem failed")?;

        let local_root = std::env::current_dir().context("Getting current directory")?;
        let remote_root = PathBuf::from(cli.remote_dir.clone());

        let blacklist_paths: Vec<PathBuf> = cli
            .blacklist
            .iter()
            .map(|s| PathBuf::from(s))
            .collect();

        let blacklist_names: HashSet<String> = cli
            .blacklist
            .iter()
            .map(|s| Path::new(s).file_name().map(|n| n.to_string_lossy().to_string()))
            .flatten()
            .collect();

        let app = Self {
            // sess,
            sftp,
            local_root,
            remote_root,
            blacklist: blacklist_paths,
            blacklist_names,
            debounce: Duration::from_secs(cli.debounce_s),
        };

        // Ensure remote root exists
        app.ensure_remote_dir(None, 0o755)?;
        Ok(app)
    }

    fn run(mut self, cli: &Cli) -> Result<()> {
        let initial = cli.initial_sync && !cli.no_initial_sync;
        if initial {
            eprintln!("Starting initial sync …");
            self.transfer_all()?;
            eprintln!("Initial sync complete.");
        }

        let (w_tx, w_rx) = mpsc::channel::<notify::Result<Event>>();
        let (m_tx, m_rx) = mpsc::channel::<Event>();

        let mut watcher: RecommendedWatcher =
            notify::recommended_watcher(move |res| {
                let _ = w_tx.send(res);
            }).context("Creating file watcher")?;

        watcher
            .watch(Path::new("."), RecursiveMode::Recursive)
            .context("Starting watch on current directory")?;

        // Thread: turn notify results into raw events for our dispatcher
        let tx = m_tx.clone();
        let _h_watcher = thread::spawn(move || {
            if let Err(e) = file_event_receiver(w_rx, tx) {
                eprintln!("[watcher] error: {e:#}");
            }
        });

        // Dispatcher loop in the main thread (has access to &mut self.sftp)
        if let Err(e) = self.dispatcher(m_rx) {
            eprintln!("[dispatcher] error: {e:#}");
        }

        Ok(())
    }

    fn transfer_all(&mut self) -> Result<()> {
        let mut queue: VecDeque<PathBuf> = VecDeque::new();
        queue.push_back(self.local_root.clone());

        while let Some(dir) = queue.pop_front() {
            for entry in fs::read_dir(&dir).with_context(|| format!("Reading {:?}", dir))? {
                let entry = entry?;
                let path = entry.path();

                if self.is_blacklisted(&path) {
                    continue;
                }

                let rel = self.rel(&path)?;
                let remote = self.remote_root.join(&rel);

                let meta = entry.metadata()?;
                let mode: i32 = (meta.mode() & 0o777) as i32;

                if meta.is_dir() {
                    self.ensure_remote_dir(Some(&remote), mode)?;
                    queue.push_back(path);
                } else if meta.is_file() {
                    self.ensure_remote_dir(Some(remote.parent().unwrap()), 0o755)?;
                    self.copy_file_to_remote(&path, &remote, mode)?;
                }
            }
        }
        Ok(())
    }

    fn dispatcher(&mut self, m_rx: Receiver<Event>) -> Result<()> {
        let mut last_tick = Instant::now();
        let mut events = VecDeque::new();

        loop {
            match m_rx.try_recv() {
                Ok(ev) => events.push_back(ev),
                Err(TryRecvError::Disconnected) => {
                    eprintln!("Event channel disconnected; exiting.");
                    break;
                }
                Err(TryRecvError::Empty) => (),
            }

            if last_tick.elapsed() >= self.debounce {
                last_tick = Instant::now();
                self.process_events(&mut events)?;
            }

            // Keep CPU calm
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    /// Coalesce many events per path into a minimal action list.
    fn process_events(&mut self, events: &mut VecDeque<Event>) -> Result<()> {
        let mut per_path: HashMap<PathBuf, Vec<EventKind>> = HashMap::new();

        while let Some(e) = events.pop_front() {
            for p in e.paths {
                // Absolutize to compare reliably; ignore errors quietly
                let full = std::fs::canonicalize(&p).unwrap_or(p.clone());
                per_path.entry(full).or_default().push(e.kind.clone());
            }
        }

        for (path, kinds) in per_path {
            if self.is_blacklisted(&path) {
                continue;
            }

            let mut actions: Vec<Action> = Vec::new();
            let mut last = Action::None;

            for k in kinds {
                let action = match k {
                    EventKind::Create(_) | EventKind::Modify(_) => Action::Transfer,
                    EventKind::Remove(_) => Action::Delete,
                    _ => Action::None,
                };
                match action {
                    Action::Transfer => {
                        if last != Action::Transfer {
                            actions.push(Action::Transfer);
                        }
                    }
                    Action::Delete => {
                        actions.clear();
                        actions.push(Action::Delete);
                    }
                    Action::None => {}
                }
                last = action;
            }

            if let Some(final_action) = actions.last().copied() {
                match final_action {
                    Action::Transfer => self.transfer_element(&path)?,
                    Action::Delete => self.delete_element(&path)?,
                    Action::None => {}
                }
            }
        }

        Ok(())
    }

    fn transfer_element(&mut self, path: &Path) -> Result<()> {
        if self.is_blacklisted(path) {
            return Ok(());
        }

        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => {
                return self.delete_element(path);
            }
        };

        let rel = self.rel(path)?;
        let remote = self.remote_root.join(&rel);
        let mode: i32 = (meta.mode() & 0o777) as i32;

        if meta.is_dir() {
            self.ensure_remote_dir(Some(&remote), mode)?;
        } else if meta.is_file() {
            if let Some(parent) = remote.parent() {
                self.ensure_remote_dir(Some(parent), 0o755)?;
            }
            self.copy_file_to_remote(path, &remote, mode)?;
        }
        Ok(())
    }

    fn delete_element(&mut self, path: &Path) -> Result<()> {
        if self.is_blacklisted(path) {
            return Ok(());
        }
        let rel = match self.rel(path) {
            Ok(r) => r,
            Err(_) => return Ok(()), // ignore paths outside local_root
        };
        let remote = self.remote_root.join(&rel);

        // Try file unlink first, then rmdir. If directory not empty, attempt recursive.
        if self.sftp.unlink(&remote).is_ok() {
            eprintln!("remote: deleted file {}", remote.display());
            return Ok(());
        }

        // If it's a directory, try to remove recursively
        if self.remote_is_dir(&remote)? {
            self.remote_remove_dir_recursive(&remote)?;
            eprintln!("remote: removed dir {}", remote.display());
        }

        Ok(())
    }

    fn copy_file_to_remote(&mut self, local: &Path, remote: &Path, mode: i32) -> Result<()> {
        eprint!("sync: {} -> {}...", local.display(), remote.display());

        let mut rf = self.sftp.create(remote)?;
        let mut lf = File::open(local)?;
        std::io::copy(&mut lf, &mut rf)?;

        // Set mode
        let stat = ssh2::FileStat {size: None, uid: None, atime: None, gid: None, mtime: None, perm: Some(mode as u32)};
        let _ = self.sftp.setstat(remote, stat);

        eprint!("DONE!\n");
        Ok(())
    }

    fn ensure_remote_dir(&self, remote_dir: Option<&Path>, mode: i32) -> Result<()> {
        let mut built = PathBuf::new();

        let r = remote_dir.unwrap_or(&self.remote_root);
        for comp in r.components() {
            built.push(comp.as_os_str());
            if built.as_os_str().is_empty() {
                continue;
            }

            match self.sftp.mkdir(&built, mode) {
                Ok(_) => {}
                Err(e) => {
                    // If it already exists (race), ignore
                    if !self.remote_exists(&built)? {
                        return Err(e).with_context(|| format!("mkdir {:?}", built));
                    }
                }
            }
        }
        Ok(())
    }

    fn remote_exists(&self, remote: &Path) -> Result<bool> {
        match self.sftp.stat(remote) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn remote_is_dir(&mut self, remote: &Path) -> Result<bool> {
        match self.sftp.stat(remote) {
            Ok(stat) => Ok(stat.is_dir()),
            Err(_) => Ok(false),
        }
    }

    fn remote_remove_dir_recursive(&mut self, remote: &Path) -> Result<()> {
        // List entries; if readdir fails, try rmdir as a last resort
        let entries = match self.sftp.readdir(remote) {
            Ok(v) => v,
            Err(_) => {
                let _ = self.sftp.rmdir(remote);
                return Ok(());
            }
        };

        for (child, stat) in entries {
            if let Some(name) = child.file_name() {
                if name == "." || name == ".." {
                    continue;
                }
            }
            if stat.is_dir() {
                self.remote_remove_dir_recursive(&child)?;
            } else {
                let _ = self.sftp.unlink(&child);
            }
        }
        let _ = self.sftp.rmdir(remote);
        Ok(())
    }

    fn rel(&self, path: &Path) -> Result<PathBuf> {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        canon
            .strip_prefix(&self.local_root)
            .map(|p| p.to_path_buf())
            .with_context(|| format!("Path {:?} is outside project root {:?}", path, self.local_root))
    }

    fn is_blacklisted(&self, path: &Path) -> bool {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if self.blacklist_names.contains(name) {
                return true;
            }
        }
        for blk in &self.blacklist {
            if path.starts_with(blk) {
                return true;
            }

            let rel_try = self.local_root.join(blk);
            if path.starts_with(&rel_try) {
                return true;
            }
        }
        false
    }
}

fn file_event_receiver(w_rx: Receiver<notify::Result<Event>>, m_tx: Sender<Event>) -> Result<()> {
    for res in w_rx {
        match res {
            Ok(event) => {
                // Only forward interesting kinds
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                        let _ = m_tx.send(event);
                    }
                    _ => {}
                }
            }
            Err(e) => eprintln!("watch error: {e:?}"),
        }
    }
    Ok(())
}

fn expand_tilde(s: &str) -> String {
    if s.starts_with("~/") {
        if let Some(home) = env::home_dir() {
            return home.join(&s[2..]).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let app = App::new(&cli)?;

    app.run(&cli)
}
