# Installing Folio

Folio is a desktop app for Windows and macOS. There are two ways to get it:

1. **Install it.** Download the installer for your computer. This is the quickest way.
2. **Run it from the source code.** Use this if the installer won't open on your computer, or if you want to change the code.

These are preview builds. They are not signed by Microsoft or Apple, so your computer will warn you the first time you open Folio. The steps below show how to get past that warning.

## Which file do I need?

| Your computer                                    | Download                        |
| ------------------------------------------------ | ------------------------------- |
| Windows 10 or 11 (64-bit)                        | `Folio_<version>_x64-setup.exe` |
| Mac with Apple Silicon (M1, M2, M3, M4 or later) | `Folio_<version>_aarch64.dmg`   |

Not sure which Mac you have? Open the Apple menu → **About This Mac**. If **Chip** says Apple M-something, use the `.dmg`. If it says Intel, there's no installer for you yet, so use [Run from source](#option-2-run-from-source) instead.

Get the files from the repository's **Releases** page. You can also get them from a run of the **Installer preparation** workflow under **Actions**, in the **Artifacts** section at the bottom of the run. Artifacts expire after 7 days.

## Option 1: Install

### Windows

1. Double-click `Folio_<version>_x64-setup.exe`.
2. If **Windows protected your PC** appears, click **More info**, then **Run anyway**.
3. Follow the installer. It installs for your user account only, so it doesn't ask for an administrator password.
4. Open **Folio** from the Start menu.

If the installer says it needs **WebView2**, let it download it. That needs an internet connection. Windows 11 and most up-to-date Windows 10 PCs already have it.

To uninstall, go to **Settings → Apps → Installed apps → Folio → Uninstall**.

### macOS

1. Double-click `Folio_<version>_aarch64.dmg`.
2. In the window that opens, drag **Folio** onto **Applications**.
3. Open **Folio** from Applications.
4. macOS will refuse the first time, saying it can't check the app for malicious software, or that Apple could not verify it. Click **Done** or **Cancel**, then:
   - Open **System Settings → Privacy & Security**, scroll down to the message about Folio, click **Open Anyway** and confirm with your password.
   - Or, in Terminal, run this once:
     ```sh
     xattr -dr com.apple.quarantine /Applications/Folio.app
     ```
5. Open Folio again. It opens normally from now on.

If macOS says **"Folio is damaged and can't be opened"**, the file isn't actually broken. Run the `xattr` command from step 4 and try again.

To uninstall, drag **Folio** from Applications to the Trash.

### Check the download (optional)

Each download comes with a `SHA256SUMS.txt`. To confirm your file isn't corrupted:

- **macOS:** in the download folder, run `shasum -a 256 -c SHA256SUMS.txt`. It should say `OK`.
- **Windows (PowerShell):** run `Get-FileHash .\Folio_<version>_x64-setup.exe`. The hash should match the one in `SHA256SUMS.txt`.

## First run

1. Folio walks you through a short setup. **Choose a folder** with your documents. Folio reads TXT, Markdown and text PDFs. Scanned PDFs need OCR, which Folio doesn't do. Your files stay where they are; Folio never moves or copies them without asking.
2. **Download the local AI models** when Folio offers them. You can skip this and do it later under **Model Lab → Local AI models**. This is a one-time download of several hundred MB and needs internet. After that, search, summaries and Ask & Act work offline.
3. Keyword search, browsing your files and the links between them work straight away, even without the AI models.

Folio keeps its index, history and downloaded models here, never inside your folders:

- **macOS:** `~/Library/Application Support/dev.folio.desktop`
- **Windows:** `%APPDATA%\dev.folio.desktop`

Uninstalling Folio leaves this folder behind. Delete it too if you want to remove everything, including the downloaded models. The installed app and `npm run tauri dev` share this folder, so they see the same folders and models.

## Option 2: Run from source

Use this when the installer won't open, you have an Intel Mac or Linux, or you want to work on Folio. It builds the app on your own computer, so the first run takes a while (often 10 to 20 minutes); later runs are much faster.

### 1. Install the tools

You need **Git**, **Node.js 22.12 or newer** (with npm), and **Rust**.

**macOS**

```sh
xcode-select --install                                           # Apple's build tools
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust (choose the default)
```

Install Node.js 22 from [nodejs.org](https://nodejs.org/) (the LTS installer), or with Homebrew: `brew install node@22`.

**Windows**

1. Install [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and tick **Desktop development with C++**.
2. Install Rust with [rustup-init.exe](https://rustup.rs/) and choose the default (MSVC) option.
3. Install [Node.js 22 LTS](https://nodejs.org/) and [Git for Windows](https://git-scm.com/download/win).
4. WebView2 is already on Windows 11. On Windows 10, install the [Evergreen WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) if it's missing.

Restart your terminal afterwards, then check that everything is installed:

```sh
node --version    # v22.12 or newer
cargo --version
```

The full list for each operating system is in the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

### 2. Get the code and run it

```sh
git clone https://github.com/duckycodess/folio.git
cd folio
npm ci
npm run tauri dev
```

A Folio window opens once the build finishes. Leave the terminal open while you use it. Close the window or press `Ctrl+C` in the terminal to stop.

The first build needs internet: it downloads the Rust libraries and ONNX Runtime.

### 3. Build your own installer (optional)

To make an installer from your copy:

```sh
# macOS (Apple Silicon)
npm run tauri -- icon src-tauri/icons/source.png --output src-tauri/target/packaging-icons
npm run tauri -- build --config src-tauri/tauri.packaging.conf.json --target aarch64-apple-darwin --bundles dmg

# Windows
npm run tauri -- icon src-tauri/icons/source.png --output src-tauri/target/packaging-icons
npm run tauri -- build --config src-tauri/tauri.packaging.conf.json --target x86_64-pc-windows-msvc --bundles nsis
```

The installer ends up in `src-tauri/target/<target>/release/bundle/`. Each operating system can only build its own installer.

### Just looking at the interface?

`npm ci` then `npm run dev` opens a browser preview at `http://127.0.0.1:1420` with sample files. It needs no Rust, but it can't open your folders or run AI models.

## Troubleshooting

| Problem                                                    | Fix                                                                                                                    |
| ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Windows: "Windows protected your PC"                       | **More info → Run anyway**.                                                                                            |
| macOS: "can't be opened" / "Apple could not verify"        | **System Settings → Privacy & Security → Open Anyway**, or run the `xattr` command above.                              |
| macOS: "Folio is damaged"                                  | Run `xattr -dr com.apple.quarantine /Applications/Folio.app`.                                                          |
| Intel Mac or Linux                                         | There's no installer yet. Use [Run from source](#option-2-run-from-source).                                            |
| Window opens blank or white (Windows)                      | Install or repair the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/), then reopen Folio. |
| `npm ci` fails                                             | Check `node --version` is 22.12 or newer. Delete `node_modules` and try again.                                         |
| `npm run tauri dev` says `cargo` not found                 | Rust isn't on your PATH. Restart the terminal after installing Rust, or run `source "$HOME/.cargo/env"` (macOS).       |
| Windows build: `link.exe` not found or MSVC errors         | Install C++ Build Tools with **Desktop development with C++**, then open a new terminal.                               |
| macOS build: `xcrun: error: invalid active developer path` | Run `xcode-select --install`.                                                                                          |
| First build fails while downloading                        | The first build needs internet for Rust crates and ONNX Runtime. Reconnect and run the command again.                  |
| AI features say a model isn't installed                    | Go to **Model Lab → Local AI models** and download the models. Keyword search works without them.                      |
| A PDF isn't searchable                                     | It's probably scanned (an image with no text layer). Folio reads text PDFs only.                                       |
| Start over completely                                      | Quit Folio and delete the app-data folder listed under [First run](#first-run). Your documents are untouched.          |

Still stuck? Open an issue on GitHub. Include your operating system, which option you used, and the exact error message or a screenshot.
