# rmote

Simple, fast SFTP directory mirror: local → remote

## Table of Contents

* [Description](#description)
* [Features](#features)
* [Prerequisites](#prerequisites)
* [Installation](#installation)
* [Usage](#usage)

  * [Command-Line Options](#command-line-options)
* [Examples](#examples)
* [Blacklist](#blacklist)
* [Debounce](#debounce)
* [Contributing](#contributing)
* [License](#license)

## Description

`rmote` watches a local directory and mirrors all file and directory changes over SFTP to a remote host. It supports initial full sync, real-time file watching, intelligent event coalescing, and a customizable blacklist of paths to ignore.

## Features

* **Initial Sync**: Perform a full upload of the local directory tree at startup.
* **Real-Time Watch**: Uses filesystem notifications to detect creates, modifications, and deletions.
* **Debounce**: Coalesces rapid events within a configurable window.
* **Blacklist**: Exclude files or directories by name or path prefix.
* **Preserves Permissions**: Remote files and directories inherit the same mode bits as local ones.
* **Recursive Deletes**: Automatically removes remote directories when local ones are deleted.

## Prerequisites

* Rust toolchain (1.65+ recommended)
* SSH key pair configured for passwordless or passphrase-protected authentication
* Remote host with SFTP enabled

## Installation

Install with cargo:

```sh
cargo install rmote
```

Compile from source with Cargo:

```sh
git clone https://github.com/Sheol27/rmote.git
cd rmote
cargo build --release
```

Optionally install to your PATH:

```sh
cargo install --path .
```

## Usage

Run `rmote` from the root of your local directory to start mirroring:

```sh
rmote --host example.com --user deploy --remote-dir /var/www/my-site
```

## Blacklist

Use `--blacklist` (or `-x`) to ignore specific files or directories by exact name or prefix. Paths matching any entry are skipped during sync and watching.

```sh
# Ignore files named "secret.json" or the entire "logs" directory
rmote -x secret.json -x logs
```

## Debounce

`--debounce-s` sets the coalescing window (in seconds) for filesystem events. Higher values group more rapid changes into a single sync operation.

```sh
# Wait 3 seconds after the last event before syncing
rmote --debounce-s 3
```

## Examples

1. **Default mirror with initial sync**:

   ```sh
   rmote --host example.com --user deploy --remote-dir /srv/app
   ```

2. **Disable initial full sync**:

   ```sh
   rmote --host example.com --user deploy --no-initial-sync
   ```

3. **Ignore `.git` and `node_modules`**:

   ```sh
   rmote -x .git -x node_modules
   ```

4. **Increase debounce to 5 seconds**:

   ```sh
   rmote --debounce-s 5
   ```

## Contributing

Contributions, issues, and feature requests are welcome! Please open an issue or submit a pull request on GitHub.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.

