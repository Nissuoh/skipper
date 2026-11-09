# Privacy

Status as of 0.1.0-test.6 (2026-11-09): Skipper is not released and nothing is published. This page says what the code stores and what it can send, so that you can judge it before the first release. No probe of the protection has been answered yet, so the protection parts are design statements, not tested facts. Statements in the present tense about code describe code that will be added to this repository; until a crate is public they are the maintainer's claims and cannot be checked. [SECURITY.md](../SECURITY.md) lists the limits in full.

The other pages: [user guide](user-guide.md), [install guide](install.md).

## In short

- Skipper has no telemetry and no analytics, and no crash reporter of its own. It reports nothing about how you use it. Windows Error Reporting may still create a dump when a program crashes, and Skipper excludes the helper's memory from such dumps. There is no Skipper server.
- The main program, `skipper.exe`, has no network code. The project checks this: the two crates that make up `skipper.exe` do not depend on the helper crates, the dependency check finds no HTTP or TLS crate in its tree, and the release step reads the list of system libraries that it imports.
- The network code is in two helper programs next to it: the update helper and the sign-in helper. The table below says when each one talks and to whom.
- What you create stays on your PC in plain files. Skipper sends none of it anywhere.
- Skipper never reads, stores, copies or sends your Claude credentials or session tokens.

## What Skipper stores, and where

| Place | What |
|---|---|
| `.skipper` in your project folder | The definitions of the project: roles and their instructions, the permissions table, the list of connections (names, addresses and rules, never a secret), the project settings and the saved layouts. These are readable text files that you can version and share. |
| `clones` in your project folder | A role's own clone of your code, in the starter team's layout. |
| `%LOCALAPPDATA%\skipper\accounts.toml` | The accounts: names and folders, never a secret. |
| `%LOCALAPPDATA%\skipper\accounts\<account>` | One Claude Code configuration folder for each account. Claude Code keeps the account's sign-in, history and transcripts there. Skipper does not read the credential. |
| `%LOCALAPPDATA%\skipper\workspaces\<project>` | The board (tasks, messages, reports, decisions, your answers), the knowledge pages and their files, a snapshot and an archive of the board, small state files, the folder where a pull request from outside is looked at, and the runs of imports. |
| `%LOCALAPPDATA%\skipper\roles\<project>\<role>` | The files that Skipper derives for a running role, and the role's own working folder. |
| `%LOCALAPPDATA%\skipper\services\<project>` | The same two things for each session that Skipper starts itself, such as the Briefing: the files that Skipper derives for it, and its own working folder. |
| `%LOCALAPPDATA%\skipper\snapshots\<project>` | Saved copies of each role's work in its clone, one store per role, the newest 20 of each. They are copies of your project's code, kept outside the project folder. The store also gets the commit history of the clone, so everything your project has committed, including files removed later, is copied as it is. A committed secret is copied with the history and stays in these copies; to remove the copies, delete this folder. Of the work that is not committed yet, files such as .env files and keys, files that look like they hold a secret and very large files are left out. |
| `%LOCALAPPDATA%\skipper\vault` | Your saved sign-ins for outside services, each sealed with Windows' DPAPI, in a folder that only you, SYSTEM and administrators can open. Never exported, backed up or synced. The sign-in that you save for an import from a service is one of these entries, under a name that begins with `import-`. A Windows Hello layer for the vault is built but switched off in this version, so no entry is sealed with it. |
| Other files in `%LOCALAPPDATA%\skipper` | Samples of the usage percentages of your accounts, the pacing settings (`pacing.toml`, which also holds your mark of an account as shared with you), the memory of use outside Skipper (`state\outside.json`: account names, times and one number each), your hardware limits, the list of your projects with their folder paths, small state files, the downloaded catalogues, and downloaded updates with backups of earlier program files. |
| `%APPDATA%\skipper` | Your language, theme and sidebar width, the last project you opened, the state of the first-steps checklist, the names of the environment variables that you allowed (the names only, never their values) and your update settings. |

Things to know about these files:

- They are protected by Windows' permissions on your own profile, nothing more. Claude Code's history and transcripts are not encrypted. A program that runs as you can read them.
- Roles that share an account share its configuration folder, and can read each other's history, including project content.
- The board and the log hold the text that your roles and you write. Do not put credentials into tasks, answers or pages.
- Saved snapshots (see the table above), the board and the transcripts can hold secrets that were committed, typed or printed. Treat them like the files of your project.
- `Skipper-Projects` in your user folder (`%USERPROFILE%\Skipper-Projects`) holds every project folder that Skipper makes itself: the folder of your first project (first start), the folder of a project made with "New project …" when you type no folder, and the folder of a project made by an import. Skipper makes `Skipper-Projects` when it is missing. For your first project and for "New project …" it makes the project's folder in it, named after the project (`Name-2` when the name is taken), and never writes into a folder that is already there. An import's folder is `Skipper-Projects\import-<eight characters>`, a new name for each import. A project folder holds the project's definitions like any project folder. A folder that you choose yourself ("Use another folder …", or a path typed in "New project …") stays where it is and is not moved into `Skipper-Projects`. Cancelling an import leaves the empty project and its folder, and a project that a later step refuses leaves its empty folder; "Remove from the list" takes a project out of the list and leaves its folder where it is. Skipper deletes none of these folders.
- Uninstalling Skipper leaves all of this in place. The [install guide](install.md) says how to remove it.

## What can leave your PC, and when

The statements in this section describe the intended design. The code that makes them true is not public yet, so until it is, read them as design statements, not as tested facts.

| Who | Where to | When | What |
|---|---|---|---|
| Claude Code, the program you installed | Anthropic, or the address you set yourself | Whenever a role works | Claude Code's own traffic. Skipper puts no proxy and no certificate in its path, changes no address and cannot see or filter it. What Claude Code shares is governed by Claude Code and your agreement with Anthropic. |
| The update helper, `skipper-update.exe` | `api.github.com`, `github.com` and `release-assets.githubusercontent.com` only, over HTTPS | While the setting "Automatically check for updates" is on, which is the default: when Skipper starts and then every 6 hours. Also when you click "Check now", with the setting on or off. Right after a check that reached the release list, it also asks for the two catalogues (models and Claude Code settings), at most once a day. The files of an update are downloaded only after you click. | It asks for the release list and downloads files. It sends no token and no cookie. Its `User-Agent` is `skipper-update/<version>`. Until the first release is published it has no release source and asks nothing, and a test build makes no request at all: it only looks into a folder on your PC. |
| The sign-in helper, `skipper-gateway.exe` | The services behind the connections that you added and allowed | When a role calls a tool of such a connection and your rules allow it, and when you sign in | The call. A saved secret goes only to the address it is bound to, over HTTPS, in its own place in the request. No redirect is followed. |
| The sign-in helper | `api.github.com`, and `codeload.github.com` for the code of a pull request | Only for the Maintainer role's work on a repository, with the sign-in that you saved | Reads pull requests and issues. It writes only after your click. |
| The sign-in helper | `api.linear.app` and `uploads.linear.app` (Linear), `api.notion.com` (the Notion API), `api.github.com` (GitHub Issues), and for attachments `file.notion.so` and two Amazon storage hosts (Notion) and `github.com` and three `githubusercontent.com` hosts (GitHub) | Only for an import from that service, after you saved a sign-in for it in the import window and started the import with your click | Reads the tasks and pages that you chose to import. The saved token goes only to the service's own address, over HTTPS. Attachment hosts get requests without any secret, and no redirect is followed. The requests only read. |
| The sign-in helper | `artificialanalysis.ai` | Only if you connect the optional source on the Models page with your own key, and at most one refresh a day | Your key, in the request header. It brings back public model data. |

During a sign-in with a service, the sign-in helper may listen for a moment on your own PC, on the loopback address only, for the return of your browser.

Neither helper reports anything about how you use Skipper. Skipper never asks for a Claude password, code or token in its own window, page or message. The only secrets it asks for are those of outside services, which you enter in Skipper's own sign-in window for the vault.

## What never leaves

- Your board, your knowledge pages, your tasks, your project files and everything that you import. Skipper sends none of it outside.
- Your Claude credentials and session tokens, because Skipper never holds them.
- Your hardware scan, your usage figures, your layouts and your project list. Nothing in Skipper sends them.
- A saved sign-in, except inside the request to the address it is bound to.

Imports from files are read by a helper in a protective box without a network.

## What you control

- "Automatically check for updates" is on by default. Turn it off and Skipper never looks for an update by itself; "Check now" still works when you click it. The files of an update are downloaded, installed or rolled back only after your click, and a test build asks nobody over the network.
- Skipper ships no connections. Each one is yours to add, and each tool of it is set to allow, ask or deny. Ask is the default.
- The Maintainer role and the optional data source work only when you set them up.
- An import from a service reads that service only after you saved a sign-in for it in the import window and started the import with your click. Delete that sign-in on the Sign-ins page when you no longer need it.
- Delete a saved sign-in on the Sign-ins page. Skipper first revokes it at the service where the service allows that.
- Opt-outs for Claude Code's own telemetry and optional traffic are set in Skipper: in the settings section "Claude Code", group "Privacy & telemetry", with the rows "Telemetry off" and "No nonessential traffic". They can apply to all roles or to one role. An opt-out that you set in Windows or in your own Claude Code settings does not reach a role, see the limits below.

## Limits

- A role in the protective box works with the internet open. A role that read a hostile page could send project data out through any allowed service. The box does not stop that.
- A role's own Claude Code session can read its account's credential, because it needs it to work. Skipper holds none and cannot close that.
- What a role puts into a tool call of an allowed connection goes to that service. The sign-in helper enforces your rules, not the role's intent.
- Windows' clipboard history may keep a token that you pasted. Skipper empties only the clipboard itself.
- Malware that runs as you can read what you can read, including the vault's files, and can drive Skipper's window.
- Skipper does not switch Claude Code's own data sharing on or off by itself. Anthropic's documentation says what Claude Code's settings do.
- A telemetry or traffic opt-out that you set in Windows or in your own Claude Code settings does not reach a role. Each account has its own configuration folder, so your own Claude Code settings do not apply in a role, and a role gets only a short list of variables from Windows, which leaves such an opt-out out. If you switched Claude Code's telemetry off outside Skipper, it is not off in a role until you set the opt-out in Skipper's settings, group "Privacy & telemetry".
