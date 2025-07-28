use anyhow::{bail, Result};
use std::fs::{self, File};
use std::net::TcpStream;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use ssh2::{Session, Sftp};
use std::collections::VecDeque;

fn connect(host: &str, port: &str) -> Result<Session> {
    let tcp = TcpStream::connect(format!("{}:{}", host, port)).unwrap();
    let mut sess = Session::new().unwrap();
    sess.set_tcp_stream(tcp);
    sess.handshake().unwrap();

    sess.userauth_pubkey_file("root", 
                              Some(Path::new("/home/sheol27/.ssh/id_ed25519.pub")),
                              Path::new("/home/sheol27/.ssh/id_ed25519"), 
                              None).unwrap();

    if !sess.authenticated() {
        bail!("Authentication failed")
    };
    
   Ok(sess)
}

fn _extact_file_mode(file: &File) -> Result<i32> {
    let m = file.metadata()?;
    let p = m.permissions();
    let mode = p.mode() & 0o777;
    Ok(mode.try_into()?)
}

fn transfer_all(sftp: &Sftp, blacklist: &Vec<&str>) -> Result<()> {
    let mut queue: VecDeque<String> = Default::default();
    queue.push_back(String::from("./"));

    while !queue.is_empty() {
        let elem = queue.pop_front().unwrap();
        let paths = fs::read_dir(elem).unwrap();

        for p in paths {
            let entry = p?;
            let path = entry.path();
            let n = path.file_name().unwrap().to_str().unwrap();

            if blacklist.contains(&n) {
                continue
            }

            println!("Processing {}", path.display());

            let mode: i32 = path.as_path().metadata()?.mode().try_into()?;
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                // TODO: better handling of the errors. It should ignore only if the folder alredy
                // exists
                match sftp.mkdir(&path, mode) {
                    Ok(_) => queue.push_back(path.to_str().unwrap().to_string()),
                    Err(_) => queue.push_back(path.to_str().unwrap().to_string())
                }
            }

            if file_type.is_file() {
                let mut rf = sftp.create(&path)?;
                let mut lf = File::open(&path)?;
                std::io::copy(&mut lf, &mut rf)?;
            }

        }
    };

    Ok(())
}

fn main() -> Result<()> {
    let dir_name = ".rmote";
    std::fs::create_dir_all(dir_name).unwrap();

    let session = connect("192.168.1.206", "22")?;
    let sftp = session.sftp()?;


    let blacklist = vec![".rmote", "target", "build", ".git"];

    transfer_all(&sftp, &blacklist)?;


    Ok(())
}
