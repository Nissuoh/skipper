# Security policy

This file does two jobs. It says how to report a problem, and it states in advance, plainly, what Skipper's protection will and will not do, so that nobody is promised more than the design can deliver. Most of the protection described below is built in code, and the text says where it is not. But no probe of the protection has been answered yet, so everything below that describes protection is a design statement, not a tested fact. (Skipper's own check of which protection tier can start a program ran on the project's own PC, Windows 11 build 26300, on 2026-11-08 and found that both the protective box and the weaker tier start a small test program there. That shows that they start. It does not show what they keep out.) Statements marked "probable" or "unverified" can only be settled by probes on a real Windows machine (design document, section 20). The design document is `docs/design/2026-10-20-skipper-design.md`; section numbers below refer to it. It follows with the code (documents first), so that path does not exist here yet.

## Reporting a vulnerability

- Use GitHub's private vulnerability reporting for this repository: open the repository's Security tab and choose "Report a vulnerability" (direct link: <https://github.com/Nissuoh/skipper/security/advisories/new>).
- If the button is missing, open a public issue with the "Design feedback" form, because blank issues are off. Set the title to "Design: security contact needed", type "security contact needed" in "Where" and in "What did you notice?", choose "A question", and tick both boxes (this request is only a contact request and holds no details). Add nothing else. The maintainer will arrange a private channel. Never put the vulnerability itself in that issue.
- Apart from the contentless contact request above, do not report a vulnerability in a public GitHub issue, pull request or discussion, and do not publish details before a fix exists.
- A security problem in Claude Code itself is not reported here. Report it to Anthropic first, as "Problems in Claude Code" below describes.
- No e-mail address is published for reports.
- Say what is affected (version or commit, Windows version, the tier shown on the role's card), the steps to reproduce, and the impact you see.
- Never include real credentials, session tokens or the contents of anyone's Claude Code config directory. Use a throwaway account and dummy data.
- Skipper has one maintainer (see `GOVERNANCE.md`), so the rest is best effort and carries no service-level promise.

## What to expect

- **Acknowledgement:** the target is 7 days (best effort, no SLA).
- **Disclosure:** the default window is 90 days from your report, and it can be extended by agreement with you. Please do not publish before the fix or before that date.
- **Fixes:** published as GitHub security advisories, with credit to you unless you decline it.
- **Good-faith research:** if you test only against your own machine and accounts, use dummy data, and stop when you find a problem, the project will not take action against you.

While no release exists, no version is supported. Once the first release exists, this file will list the supported versions.

**Status as of 0.1.0-test.6 (2026-11-09): in active development, not released.** No build has been published and the source code is not in this repository yet (it follows in small reviewed pull requests; see the [README](README.md) and [docs/code-arrival.md](docs/code-arrival.md)), so there is no released version that could be vulnerable. Statements in the present tense about code describe code that will be added; until a crate is public they are the maintainer's claims and cannot be checked.

The `main` branch is protected: it takes changes only through pull requests, with no force-push and no deletion (see the [working agreement](docs/github-workflow.md), rule 7).

## Words used here

This page is meant to be readable without the design document, which follows with the code (documents first) and is not in this repository yet.

- **The four hard locks** are four rules that are fixed in code and are not settings: (1) no account is put at risk, so Skipper never reads, stores, copies or sends Claude credentials or session tokens; (2) nothing runs invisibly, so every role has a visible panel or card; (3) nothing reaches the main branch or the internet without a person's click in Skipper's own window, and a message from a role never counts as a yes; (4) the PC does not freeze, because Windows always keeps a reserve of memory and cores that can never be zero. The README describes them under "The four hard locks".
- **The one route through which Skipper opens files** is a single piece of code that every file read of Skipper goes through, so that one list of rules is applied in one place.
- **The deny-list** is the list of files and folders that this route refuses to open, for example the credential files of Claude Code and of git, SSH and cloud tools.
- **A probe** is a test on a real Windows machine that settles a statement marked "probable" or "unverified". See [docs/probe-guide.md](docs/probe-guide.md).
- **Section numbers and ids** such as "section 9.2", `ACC-5`, `MC-11` and `PRB-40b` refer to the design document that follows with the code. They are only extra references: each point below says in words what it means. Until the design document is published, decisions about these rules are recorded in the issue or pull request that makes them.

## Scope

In scope:

- Any way to get around one of the four hard locks (see "Words used here"): a final approval that does not come from a person's click in Skipper's own interface, Skipper pushing or publishing by itself, a process that runs without being shown, or a resource reserve below its floor.
- Skipper reading, storing, copying or sending Claude credentials or session tokens, or any other credential outside its vault, including a bypass of the deny-list on the one route through which Skipper opens files (both described in "Words used here").
- A role reaching files, other roles or other accounts' config directories beyond what its tier claims to prevent.
- The connection between roles and the core: anyone but the intended role and the person being able to connect, or any network port being opened.
- Text that one role wrote steering another role or Skipper itself: a board entry, task, report, evidence, knowledge page or reply of Skipper's own tools or hooks that Skipper delivers without the frame that names its true sender, a forged frame that Skipper does not escape, or text that makes Skipper grant a right, change a rule, change a task's state outside the board's rules, or record an approval (design section 8.3).
- A start command or environment that carries something the design forbids, for example a print-mode or permission-mode argument, or a variable outside the allow-list; a start environment that drops or changes a sign-in variable the person set (`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_BASE_URL`, or a cloud switch), or that passes a cloud credential the person did not allow by name; or Skipper storing, logging or showing the value of such a variable (design section 9.2, ACC-5, MC-11).
- A tier, network state or other label that looks safer than the tier is.
- Skipper's own programs, installer, release files, build scripts and dependency handling, including anything that lets a tampered or blocked dependency into a build.
- Skipper's own handling of connection rules (outside MCP servers per role, design section 12.9): how it writes, shows and labels tool rules, derives each role's MCP file and treats secrets. Bugs in the outside MCP servers themselves belong to their own projects.
- Skipper's vault and sign-in helper (design section 12.11): a secret that reaches a role, a log, a view, a Windows error report or crash dump, or any file outside the vault folder; a secret sent to any address but the one it is bound to; a way to unlock the vault, answer an ask, start a sign-in or change a rule from anything but Skipper's own window; a saved value that can be shown, copied or exported; a flaw in the helper's parsers or its link to the core; a vault folder that Skipper uses although its type, owner or access list is wrong.

Out of scope:

- Claude Code itself and Anthropic's services. Report those to Anthropic, as "Problems in Claude Code" below describes.
- Windows, WSL, Docker and other third-party components, unless Skipper uses them wrongly.
- The known limits listed below. A report that shows a way past a documented limit is in scope; the limit itself is already known.
- An attacker who already controls the whole PC, or a process that remote-controls Skipper's interface as the signed-in Windows user.
- Choices a person made on purpose, for example allowing extra network names for a role, turning on a permission-skipping mode inside Claude Code, or setting an API key, token or base address in their own environment, which then reaches their roles unchanged, or allowing a cloud credential by name, or turning on one of the four per-role feature switches (background sessions, agent teams, `/loop`, channels; design section 8.3).
- Questions about whether a setup fits Anthropic's terms. That is not a security question, and it is not legal advice.
- A role's own Claude Code session reading its account's Claude credential: a documented limit ("Saved sign-ins" below).
- A Claude model following instructions that it read in text which Skipper delivered with its true sender named. That is a limit of the model, listed under "Text between roles" below; a way to make Skipper drop or forge the frame is in scope.

## Supply chain today

Skipper's dependency checks are strict about versions and not yet about review, so read them with these facts in mind:

- All dependencies are exact pins, `Cargo.lock` is committed, builds run offline, and there is no git source.
- No dependency has been audited by this project. Every registry package is on the exemption list in `supply-chain/config.toml`, so a green `cargo vet` means "known and pinned", not "reviewed". The check tool prints how many packages are audited and how many are exempted.
- The audits of other organisations that the configuration imports count only once `supply-chain/imports.lock` is committed. That file is not in the repository yet.
- The 7-day wait before a new crate version is used is waived for the current pin set (owner waivers, recorded in `supply-chain/pins.toml`) until version 1.0. From 1.0 on, every new version waits 7 days.
- The exemptions are to be cut, security-relevant crates first, before 1.0. Until then this list is the honest state, and nothing here should be read as an audit result.

## Problems in Claude Code

Skipper runs the unmodified Claude Code, so some problems found while using or testing Skipper are problems in Claude Code itself.

- Report a security problem in Claude Code to Anthropic first, through the route in the section "Reporting security issues" of Claude Code's [security page](https://code.claude.com/docs/en/security), which points to Anthropic's HackerOne submission form. Anthropic asks that such a problem is not disclosed publicly and that it gets time to address the problem first.
- Do not publish it in this repository, in an issue, a pull request or a probe record before Anthropic has had that time.
- If a problem touches both Skipper and Claude Code, report the Claude Code part to Anthropic and the Skipper part here, and say in each report that the other one exists.
- The same rule binds the project: when Skipper's own probes or tests find a security problem in Claude Code, the maintainer reports it to Anthropic first and records only "reported to Anthropic" until Anthropic has had time to address it. (maintainer's call)

## Honest limits

**Skipper's protective box is weaker than a Linux container, because the Windows kernel is shared. (owner decision)** The interface must never look safer than the tier is: every role card shows the tier as a word and a symbol, next to the network state. (maintainer's call)

### The protective box, tier by tier

| Tier | What it is | Label shown |
|---|---|---|
| T1 `native-box` (default) | One job object and one AppContainer per role, and a pseudo console owned by Skipper. No administrator needed. | "Files protected · Internet open" |
| T1b `native-box-weak` (fallback) | Job object, restricted token (limits writing only: to the role's own folders and to objects that grant Everyone write), access lists. | "weak: reads everything you can read, including other accounts' config directories · Internet open" |
| T2 `net-filter` (not built; planned for Stage 2, Windows 11 only, after its probe) | T1 plus an address filter. | "Network by address list (coarse)" |
| T4 `wsl-linux` (not built; planned for Stage 2, only where WSL is already present) | One WSL container session per Claude account. | "strongest tier in Skipper – not promised as a security boundary by Microsoft" |
| `docker` (not built; voluntary, Stage 2 at the earliest) | The person's own container. | "Protection as your container" |

Further labels: "not verified" until a tier's probe has passed. The interface already holds the texts for T2, for T4 and for "not available on this PC" (T4 where WSL is missing), but no code starts a role in those tiers yet. Skipper starts no role on a Windows older than Windows 11 (build 22000): the role does not start and says so, so Windows 10 gets no protection label at all. A further voluntary tier with its own Windows account (T3) may never be built (open – owner decides).

What the tiers do not do:

- **T1** (probable; needs probes). Each role gets its own identity, so only explicitly granted folders are reachable: the project folder, the account's config directory, the role's own work folder, the `claude` program folder (read and execute) and the role's own pipe. But T1 does not hide what Windows lets every AppContainer read, such as the Windows folder, Program Files and parts of the registry. It does not separate roles that share an account: they share the account's config directory and can read each other's history and transcripts, including project content. It does not stop a hijacked role from sending its own credentials and the project over the open internet. It gives no kernel-level boundary. And it is not yet proven that `claude`, Git and PowerShell run inside an AppContainer at all.
- **T1b.** Because it limits only writing, a role can read every account's config directory, transcripts included, that the person can read, and send them out. Its write limit also has one opening on purpose: the role's token is restricted to its own identity and to Everyone, because with its own identity alone no real program (Claude Code, Git, a Windows library for encryption) started in this tier on the project's own PC. So a role in T1b can write to its own folders and to every file, folder, registry key or system object whose access list grants Everyone write access. Skipper never makes such an entry, but other software does: on the project's own PC a few shared folders and registry keys of installed programs were found. The cause of the failure without Everyone is probably a system object that those programs open for writing and whose access list names Everyone; which object that is has not been identified. A role at Low integrity still cannot write to anything that carries a higher integrity label; where Low does not start `claude`, the token stays at Medium and the role card says so. With the Windows unlock it also reads and decrypts any saved sign-in, so no role starts in T1b while the vault holds one ("Saved sign-ins" below). It is therefore allowed only while exactly one account is registered, or with several accounts only if a probe shows that other accounts' config directories can be locked against reading too. It never starts a role in a "skip all permission prompts" mode; if the person set one inside Claude Code, the panel shows a red warning and Skipper changes nothing.
- **T4.** Microsoft's own documentation says WSL is not a security sandbox for running untrusted code. T4 is never labelled above "medium to strong", and Skipper adds its own inner shell on top of the virtual machine for that reason.
- **Config directories, snapshots, the board and transcripts are secret stores.** Claude Code's history and transcripts are not encrypted at rest, so an account's config directory is protected only by operating-system permissions. The same holds for Skipper's saved snapshots (they also copy the commit history), its board and its transcripts: a secret that was committed, typed or printed can sit in them.
- **Elevated steps.** Microsoft's published guidance for the AppContainer route names two administrator steps. Skipper never runs them and never installs an administrator service or scheduled task. If the probe shows they are needed, the fixed fallback is the weaker T1b, labelled as such.
- **Background sessions.** Claude Code's background sessions run through a per-user service that outlives the shell. If that service already runs outside the box, a role could hand work to it, which would be an escape; with several accounts it could also mix accounts (unverified). They are therefore off by default, switched off in each role's own settings file, and only the person can switch them on per role, which the role then shows (design section 8.3). With the switch on, they may keep running after Skipper's window closes, background sessions inside a box show "not verified" until a probe has passed, and with several accounts the panel shows a red warning.

### Text between roles

Skipper's job is to pass text that one Claude Code session wrote to other sessions, some of them on other accounts. A role that read a hostile web page, issue or file can write a report or page that tries to steer the lead or another role. Skipper therefore delivers every such text as information with its true sender named, never as an instruction from the person; hook replies carry counts and entry numbers only, never the text; and no text grants a right or counts as an approval (design section 8.3). This lowers the risk, but it cannot make a Claude model immune to prompt injection. The backstop stays the third hard lock, held by Skipper's own means (see below); Claude Code's permission prompts are not counted on, and in 1.0 every role starts with auto mode switched off.

### Known exfiltration paths

Skipper puts no proxy into the path of `claude`: no proxy variable, no certificate of its own, no redirected base address of its own. A base address that the person set themself (`ANTHROPIC_BASE_URL`) is passed on unchanged, like the other sign-in variables (design ACC-5). The network boundary is built only from filters, so TLS stays end to end between `claude` and the address it uses (Anthropic, unless the person set a base address), and Skipper cannot see or filter the content of that traffic. (maintainer's call)

| Tier | How the network is limited | What it does not stop |
|---|---|---|
| T1 | Internet client access only, without private-network access; the home network and the PC itself (loopback) are probably unreachable (needs a probe). | The internet is open. A role can send data to any host. |
| T1b | No network limit. | Everything is open. |
| T2 | An address filter: the name list is resolved at a role's start. | Addresses only, no names. Shared CDN addresses allow more than intended. Changed addresses are noticed only at the next start. |
| T3 | The same list through a kernel network filter. | The same as T2. |
| T4 | A DNS guard inside the virtual machine. | Open until its probes pass. Afterwards exact by name at resolution time; two names that share an address share the opening. |

Only T1 and T1b are built, so only the first two rows apply today; the other rows describe tiers that are planned. For every tier with a list, the filters stop mistakes and unintended targets, not targeted exfiltration. Data can still leave through any allowed service, including `api.anthropic.com` itself. A published 2025 security-research result showed an injected task uploading data with the attacker's own API key through the files interface of a code-execution product; whether the same works against Claude Code is unverified.

The name list is split. Anthropic's own names, telemetry included, are on by default, and Skipper blocks none of them, so as not to conceal anything from Anthropic. Third-party services such as a Git host or package registries are switchable per role, visibly, because some of them accept uploads made with someone else's credentials. (open – owner decides)

Credentials: Skipper itself never reads, stores, copies or forwards Claude credentials or session tokens, or any other credential outside its vault (git, SSH or cloud credential files, for example); the saved sign-ins of outside services are the one exception, which the vault and the sign-in helper keep (see "Saved sign-ins" below). A role's own Claude Code session, however, can send its own credentials over the network in T1 and T1b, because the internet is open there. An API key, token or base address that the person set in their own environment reaches the role's Claude Code unchanged, and so does a cloud credential that the person allowed by name, so a role's own shell can read it; Skipper only checks its name for the notice on the role and never looks at, keeps or shows its value. A cloud credential can open the person's whole cloud account, so it is never passed on without that opt-in (design MC-11).

### Saved sign-ins: the vault and the sign-in helper

Skipper keeps the sign-ins and tokens of outside services, such as the services behind a role's connections, in its own vault (design section 12.11). The person signs in once, in Skipper's own window, and a role uses the service through Skipper without ever seeing a password, token or key. A Claude account is different: its credential stays in Claude Code's own store, and Skipper only runs Claude Code's own sign-in in its panel.

What the vault and the helper are meant to protect against:

- **Files.** Each saved sign-in is sealed with Windows' DPAPI in a folder that only the person, SYSTEM and administrators can open. No protective box gets that folder, and Skipper's own file door denies it. The vault is never exported, backed up or synced; on another PC the person signs in again. When the folder is a reparse point, or its owner or access list is wrong, Skipper uses nothing from it, renames it aside by itself (`vault.refused-<date>`, never read and never deleted by Skipper) and starts a fresh vault, and the person signs in again.
- **One process.** Only the separate program `skipper-gateway.exe` decrypts a secret, only while a call needs it, and wipes it afterwards. The main program has no network or TLS library and never holds a secret; a typed token goes into the helper's own prompt, never into the main program.
- **One destination.** A secret is sent only to the address it is bound to, over HTTPS, in its own place in the request. No redirect is followed, and an echo of the secret in an answer is removed before the answer reaches a role.
- **Every call decided by Skipper.** Claude Code's rules are not the only fence for these connections: Skipper's core decides each call (grant, known and reviewed tool, protection tier, lock, sign-in, rule, first use) and the person answers an ask in Skipper's window. A role sees only the tool list that the person reviewed. Logs hold names and counts, never a value.

What they do not protect against:

- **A role's own Claude credential.** The `claude` of a role must read its account's credential to work, and the role's shell runs as the same identity: what Claude Code can read, a determined role can read. Skipper holds no Claude credential and cannot close this; its guard against reads of credential file names is a hurdle only.
- **What an allowed tool can do.** An injected role can do whatever an allowed tool can do: the helper enforces the person's rules, not intent. A person's yes on an ask is only as careful as the person; showing the full arguments as role data, offering no recommended answer and allowing "once" only lowers the risk and does not remove it.
- **Removal of echoes.** It catches the exact secret and its raw, base64, percent-encoded and hex forms in the decoded answer, not a form that the service transforms in another way.
- **A malicious service.** It can lie in its results and steer a role within what the rules allow, and it can keep and misuse its token until revoked. Skipper cannot inspect its code. A parser bug in the helper would expose the secrets in its memory, because in this version the helper runs outside a protective box.
- **The weak tier (T1b).** A role in T1b runs as the person and could read and decrypt the saved sign-ins itself with the Windows unlock, so no role starts in T1b while the vault holds an entry. With the optional Windows Hello layer a T1b role may run, but the vault stays locked until it has ended. That layer is not in effect today: the Windows Hello layer is built in the sign-in helper but is switched off until its probe (PRB-96) has run, and the setting that asks for it is refused. Until then, no role starts in T1b while the vault holds a saved sign-in.
- **The open internet.** Tier T1 leaves the internet open, so project data can leave without any connection.
- **Certificate revocation.** The helper's HTTPS connections do not check whether a server's certificate was revoked (WinHTTP's revocation check, `WINHTTP_ENABLE_SSL_REVOCATION`, is off today), so a service that presents a certificate revoked after a key theft is still trusted until the certificate expires; this is recorded as the probe fact PRB-115 (documented, not run).
- **Malware on the PC (threat 4 of the design).** A program that runs as the person, outside every box, can decrypt every entry with the default unlock, because DPAPI decrypts for any program of the person; with the Windows Hello layer, once that is switched on, it cannot without the person's gesture. It can also read the helper's memory while the vault is unlocked, record keys and the clipboard while a token is pasted, read Claude Code's own credential file, and drive Skipper's window. Malware with administrator or SYSTEM rights gets everything. This holds for every local password manager.
- **A local connector (a later version).** A local program brokered by Skipper would hold its secret in its environment and could send it anywhere it can reach, because destination binding does not apply to it. The code for it is in the sign-in helper, but the app refuses such a connection for now, and its probe (PRB-91) has not run.
- **The clipboard.** Windows' clipboard history may keep a pasted token; Skipper empties only the clipboard itself.
- **The pasted sign-in address.** In the paste route of a sign-in, the pasted address passes the main program once, as a protected value. The window's own buffer is made once with room for the longest address, so typing never leaves an old buffer behind, and it is overwritten when the window closes; the interface library's undo points are emptied while the field shows. The copies that the interface library makes of its input for a frame are freed but not wiped.
- **Memory dumps.** The helper's memory is excluded from Windows error-report dumps (Skipper has no crash reporter of its own), but a `procdump -ma` dump or a Task Manager dump may still contain it.
- **Connectors of a Claude account.** The connectors of a role's Claude account are switched off, but until the probe PRB-40b shows that they are absent, they may still reach a role outside the vault.
- **A forged hook.** It can lie about its own role's state, which makes noise in Needs you and holds no secret.

These are design statements, not tested facts: the probes PRB-40b and PRB-91 to PRB-98 of design section 12.11.9 settle them on a real Windows machine.

### What the lock on the main branch and the internet can and cannot guarantee

The third hard lock (LOCK-3 in the design document, section 5.3) says: nothing reaches the main branch or the internet without a person's yes.

What it guarantees, and what tests are to prove:

- A final approval is accepted only from a command of Skipper's own interface, inside the app process, given by a person. It is never accepted through a pipe, socket, tool, hook or file, and Skipper's tool server has no tool for it. A message from a role is never consent.
- A git pre-push template asks the core whether a commit was approved by a person; only the core answers, and the approval is bound to that commit.
- The main program, `skipper.exe`, has no network library, so it cannot push or publish by itself. The network code is in two separate helper programs: the update helper, which looks for updates by itself while the setting "Automatically check for updates" is on (it is on by default), asks for the two catalogues of models and of Claude Code settings at most once a day, and downloads, installs or rolls back an update only after the person's click, and the sign-in helper, which makes the calls of the connections that the person granted, the requests of the maintainer role to the code host and the reads of an import that the person starts from a service. If the person connects the optional source of model data on the Models page with their own key, the sign-in helper also asks `artificialanalysis.ai` for public model data, at most once a day. [docs/privacy.md](docs/privacy.md) lists every destination, and when each one is asked. Later network features are off by default and act only on a click. From Stage 2, a flow places a human approval step before any publishing action; flows are not built yet.
- Tests send a final approval through every other route (pipe, tool, hook helper, status line, edit of the event log) and expect refusal each time; check the pre-push behaviour; and check that the main binary has no HTTP or TLS dependency.

What it cannot guarantee:

- It covers what Skipper does and the approval ladder, and it does not count on Claude Code's permission prompts: in 1.0 every role starts with auto mode switched off, so a permission prompt that Claude Code raises waits for the person, and no lock rests on the answer. What a role does inside its own Claude Code session, for example a `git push` through the session's shell tool, is held back only by Skipper's own means: the pre-push template where it reaches git, Skipper's hooks, the rule that only the core runs the step into the main branch after a person's click, and the network filters of the higher tiers. T1 leaves the internet open.
- Claude Code's mods (plugins that run code inside Claude Code) can approve a tool call that a deny rule or one of Skipper's hooks refused, when the session has neither managed settings nor a Team or Enterprise sign-in. A role whose connection has deny rules therefore does not start while a mod would load in it (design section 8.3). Skipper cannot see inside a mod or plugin; one that wakes an idle role or messages another session is not supported.
- A git hook runs on the machine of whoever pushes. Anything that can change the repository's local git settings, or reach the remote some other way, can skip it, and a repository without the template has no check at all.
- The sign-in helper speaks to the network for the connections that the person granted. What a role can do through an allowed tool of a connection is governed by the person's tool rules, and tools that publish, merge or delete are never allow by default (design section 12.9).
- It protects against roles and the programs they run. It does not protect against someone who controls the whole machine, or against a process that remote-controls Skipper's interface.
- A person's yes is only as careful as the person. The lock makes sure a yes is needed; it cannot make it a good one.

The other three locks (no account is put at risk, nothing runs invisibly, the PC does not freeze) are described in section 5. Section 5 states an honest limit only for the first of them (see "Credentials" above). Limits that touch the other two, such as Claude Code's background sessions, are listed under "What the tiers do not do", and the memory reserve floor is a proposal until its stress probe has run.

## Changes to this file

This file changes with the design. When a probe settles a "probable" or "unverified" statement above, the statement is updated, and a limit is never removed without its test. Changes are made by pull request with a reason.
