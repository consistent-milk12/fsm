# File System Manager (fsm)

`fsm` is a powerful and intuitive terminal-based file manager built with Rust. It aims to provide a fast, efficient, and user-friendly experience for navigating and managing your file system directly from the command line.

## Features (Planned/In Progress)

*   **Fast Navigation:** Quickly browse directories and files.
*   **File Operations:** Copy, move, delete, rename files and directories.
*   **Search Functionality:** Efficiently find files by name or content.
*   **Command Palette:** Access common actions and commands with ease.
*   **Customizable:** Theming and keybinding options.
*   **Cross-Platform:** Designed to work on Linux, macOS, and Windows.

## Installation

To install `fsm` on your system, you will need to have Rust and Cargo installed. If you don't have them, you can install them by following the instructions on the official Rust website: [https://www.rust-lang.org/tools/install](https://www.rust-lang.org/tools/install)

Once Rust and Cargo are set up, follow these steps:

1.  **Clone the repository:**

    ```bash
    git clone https://github.com/consistent-milk12/fsm.git
    cd fsm
    ```

2.  **Install the binary using Cargo:**

    ```bash
    cargo install --path .
    ```

    This command compiles the project and places the `fs` executable in your Cargo binaries directory (usually `~/.cargo/bin`), which should already be in your system's `PATH`.

3.  **Verify the installation:**

    You should now be able to run `fsm` from any directory in your terminal:

    ```bash
    fs
    ```

## Uninstallation

`cargo install` does not provide a direct uninstall command. To remove the `fs` binary, you need to manually delete the executable file.

1.  **Locate the binary:**

    The `fs` executable is typically located in `~/.cargo/bin/`.

    You can confirm its location using:

    ```bash
    which fs
    ```

2.  **Delete the binary:**

    ```bash
    rm ~/.cargo/bin/fs
    ```

## Updating `fsm`

To update `fsm` to the latest version from the repository, navigate to the cloned repository directory and run:

```bash
git pull
cargo install --path . --force
```

The `--force` flag ensures that Cargo recompiles and replaces the existing `fs` binary with the updated version.