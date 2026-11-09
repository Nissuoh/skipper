# Install guide

Status as of 0.1.0-test.6 (2026-11-09): nothing is published yet, so there is no download. This page says how the installer works as the code builds it, so you know what to expect when the first release exists. Windows only.

The other pages: [user guide](user-guide.md), [privacy](privacy.md), [tested Claude Code versions](tested-versions.md), [changelog](CHANGELOG.md).

## What you need

- Windows 11 or newer. Skipper starts no role on an older Windows.
- Claude Code, version 2.1.295 or newer, in its native Windows install, installed by you. Skipper starts only `claude.exe` from your search path, not plain `claude`. An npm install, which leaves `claude.cmd` or `claude.ps1`, is not accepted. Neither the setup nor Skipper downloads or installs Claude Code. Skipper looks for it at the first start (see the [user guide](user-guide.md)).
- A Claude subscription of your own. The README's section "Which subscriptions" says what has been checked.
- Git for Windows, with `git.exe` in a folder on the search path. Skipper uses it to make each role's clone of your code, to save copies of the roles' work and for the final approval. Without it, Skipper cannot make a clone, saves no copies and refuses the final approval.
- Memory for the roles you want to run, about 4 GiB each.

You do not need administrator rights, Docker or a restart.

## The files you download

The installer is a setup file of the release, `skipper-setup-<version>.exe`. It works together with two more files of the same release:

- `skipper-<version>.pack`, which holds the program files. The setup reads them from this file.
- `SHA256SUMS.txt`, the list of checksums.

Put all three in one folder. The setup refuses a folder that holds no pack or more than one pack, and it refuses a pack that does not belong to the setup file next to it.

Keep the file name of the setup as it is. The program takes itself for the installer only when it is started as `skipper-setup-<version>.exe`, and a name with a space or brackets, such as the one a browser makes for a second download, does not count. Rename it back, or start it from a command prompt as `<file> install`.

## Check the download

- In PowerShell, `Get-FileHash <file> -Algorithm SHA256` prints a checksum. It must equal the line for that file in `SHA256SUMS.txt`.
- The setup does the same for the pack when `SHA256SUMS.txt` is there, and stops if they differ. This catches a damaged download. It cannot catch a changed release, because both files come from the same place. The README's section "Where to get Skipper" says how to check a release against GitHub.
- Builds are unsigned at first, so Windows may warn before it runs the setup.

## Install

1. Close Skipper if you like. You do not have to: if it still runs, the setup asks whether to close it (see "Installing again: update and repair").
2. Double-click `skipper-setup-<version>.exe`.
3. Read the question and answer Yes. It names the version and the folder, and it says what is described below.
4. A message says that Skipper is installed in the folder it names, and that you can start it from the Start menu. The setup does not start Skipper by itself: start it from the Start menu.

The questions and messages are in English or German, as Windows shows its own language. They are part of the setup file itself, so even a setup file that stands alone, with no other file of its release beside it, shows its messages in words (see "When the setup refuses").

## What the installer does

- **The program folder.** By default `%LOCALAPPDATA%\Programs\Skipper`. It holds the six programs, the folders `data`, `lang` and `licenses` (`licenses` holds Skipper's own `LICENSE` and `THIRD-PARTY-LICENSES`, the texts of the third-party packages that ship), and a small record, `install.json`, that says which version was put there and that a Start menu entry was made.
- **A Start menu entry.** The shortcut `Skipper.lnk` in your own Start menu folder.
- **An entry in Windows' list of installed apps.** It is written in your own part of the registry (`HKEY_CURRENT_USER`). It shows the name "Skipper", the version and the size, and it knows how to uninstall. It has no "Modify" or "Repair" button, because running the setup again is the repair.

## What the installer never does

- It needs no administrator rights.
- It writes nowhere outside your own user folder: only the program folder and your own Start menu folder, and your own part of the registry.
- It adds no start-up entry, no service, no scheduled task, and it changes neither the search path nor any environment variable.
- It does not touch your Skipper data, and it makes no data folder.
- It closes a running Skipper only after you answer Yes to its question, and never with `--quiet`. It never starts Skipper, and it installs nothing of Claude Code. The uninstall never closes Skipper.

The build of a release reads the installer's source files for start-up entries, services, scheduled tasks, changes of the search path or of variables, machine-wide registry keys and folders, and requests for administrator rights. It stops if it finds one.

## Another folder, and scripts

The setup takes three options, and `install` before them is optional under the setup's own name:

- `--dir <folder>` installs there instead. The folder must lie below your user folder. Your user folder itself, the application data folders and Skipper's own data folders are refused.
- `--from <folder>` is where the pack is. Without it, the folder of the setup.
- `--quiet` leaves out every question and message.

Every run prints one line and ends with a code: `install: ok <version>` and 0 when it worked (`install: ok <version> repaired` when it also repaired a folder), `install: failed <key>` and 1 when it was refused or failed, `install: cancelled` and 2 when you answered No. The key names the reason in full, for example `installer.error.running` when Skipper runs.

## Installing again: update and repair

Run the setup of a newer release over the same folder and it replaces the program files. Run it over a folder that an update left half finished and it puts the folder right again. In both cases your data is not touched.

If Skipper runs when you start the setup, the setup asks "Skipper is running. Close it now so the setup can go on?" (not with `--quiet`). On Yes it asks the window of the Skipper that runs from the install folder to close, as if you had clicked its close button, and it waits up to 60 seconds for Skipper to end, with a message that says so. If Skipper has working roles, it asks its own question first, and you answer that in Skipper's window. The setup never ends a process by force. If Skipper is still there after 60 seconds, the setup says so and changes nothing. With `--quiet` the setup never closes anything and refuses at once while Skipper runs. A Skipper that runs from another folder holds the same lock and is not closed by the setup: the message asks you to close it yourself.

Skipper's own update button (see the [user guide](user-guide.md)) puts a newer version in place the same way the setup does: it replaces the program files and leaves your data. In a folder that has an install record it also writes the new version into that record and into Windows' list of installed apps, so "Installed apps" shows the new version too.

A folder with an install record is taken over. A folder without one is taken over only when it holds nothing but Skipper's own entries (the programs, the folders `data`, `lang` and `licenses`, and the leftovers of an update) and one of them shows that Skipper made it: a `skipper.exe`, or the marker or a program file in the leftovers. Any other folder is refused and left exactly as it was, with a message that names an entry in it. Choose another folder or move your own files out.

## If an update was cut off

An update that is interrupted at the wrong moment, for example by a power cut, can leave Skipper's folder without a `skipper.exe`. If you installed with the setup, run it again. It removes the leftovers of the cut update, puts the program of the release in place and says that it repaired the folder. Anything in the leftovers that is not Skipper's goes to a folder called `.install-kept` instead of being deleted.

Without the setup, open the folder `.update-old` inside Skipper's own folder, move `skipper.exe` up into Skipper's own folder and start Skipper. It puts the rest back.

## Without installing

The build folder can also be used as it is: start `skipper.exe` from it, and keep the folders and programs beside it together. Nothing is installed then, and nothing is added to the Start menu or the list of installed apps.

## Uninstall

Open Windows Settings, Apps, Installed apps, choose Skipper and click Uninstall. You can also run `skipper.exe uninstall` from the program folder (add `--quiet` to leave out the question).

- It asks first, and it refuses while Skipper runs. It never closes Skipper.
- It first takes back what the protective boxes of the roles made: the access entries and the Windows profiles. If something is left, it stops and leaves the program in place. Close every role and Skipper, then try again.
- Then it removes the program files, the Start menu entry, the entry in the list of installed apps, the install record and, if nothing else is in it, the program folder.
- The program cannot delete the folder it runs from, so it copies itself to your temporary folder, as a file named `skipper-uninstall-<number>-<number>.exe`, and lets the copy do the work. A program cannot delete the file it is running from either, so at its very end the copy hands its hold on its own file to `waitfor.exe`, a small program of Windows itself (in its System32 folder). That program waits for at most 8 seconds and ends, and Windows deletes the copy when it ends. So the copy can still be in your temporary folder for a few seconds after the uninstall has finished. If you find such a file later and no uninstall is running, it is only a copy of the program, and you can delete it. This hand-off is in the code of the installed program, so it holds only for an installation made from a build that has this fix. An installation made from an earlier build (the first test build was one) has the old code, and its uninstall still leaves the copy in your temporary folder. Run the setup of a newer build over it first, and the next uninstall is clean.

What stays:

- **Your data**, with everything in it: `%LOCALAPPDATA%\skipper` and `%APPDATA%\skipper`. They hold your boards, knowledge pages, account folders, saved copies of the roles' work, saved sign-ins and, in the folder `updates`, backups of earlier program files. Delete them yourself if you want them gone. This cannot be undone. The uninstall only tells you where they are.
- Your project folders. The uninstall never looks at them.
- Files in the program folder that are not Skipper's, and the folder `.install-kept`. The message names them, and the program folder stays then.

Claude Code and your Claude sign-ins are not Skipper's and are not touched.

## When the setup refuses

Every refusal says what happened and what to do, and most of them change nothing. The common ones:

- Skipper is running and did not close within 60 seconds, or you used `--quiet`, or it runs from another folder: answer its own questions, close it and wait until its window is gone.
- No pack, or more than one, next to the setup: keep the setup together with the files of its release. A setup file that stands alone in a folder says this in words: "Next to the setup, in <folder>, there is not exactly one release file of Skipper (skipper-<version>.pack)." and "Keep the setup file together with the files of its release in one folder, and download the release again if a file is missing." (English, or German as Windows shows its own language).
- The pack is damaged or does not match the checksum, or does not belong to the setup: download the release again, all files from the same page.
- The folder is not below your user folder, or holds files that are not Skipper's: choose another folder.
- A file step failed: close programs that hold files in the folder, check your virus scanner and run the setup again. It finishes what it left.
