# Skipper glossary

This glossary fixes the words Skipper uses for its parts, so that everyone means the same thing when they use them. Code, documentation, issues and the user interface all use these terms, and the "Avoid" lines name words that mean something else here or cause confusion.

**Status: maintained, binding for wording, as of 0.1.0-test.6 (2026-11-09).** 0.1.0-test.6 is the version number of the sixth test build (see the README). Skipper is in active development and not released, so every entry describes intended meaning, not released software. An entry with no mark describes something that exists in the code. That does not mean it is proven on a real Windows machine, because no probe counts yet. The marks say where the code is behind the entry: *(not built)* means that the code has none of it yet and *(partly built)* means that only a part of it exists. The code names under "Code:" were written before the code and are proposals: where one differs from the code, the code wins. Skipper starts empty of data: it ships no tasks, accounts or knowledge of anyone's project. It offers one optional starter team of editable roles (see Starter team); apart from that, every role name used in this list is only an example. Terms are listed alphabetically.

---

## Terms

### Account
One sign-in of Claude Code, kept in its own config directory at exactly one location. Every role points to exactly one account, and one account can serve one role or several. Sign-in happens through Claude Code's own flow, which Skipper runs in its own sign-in panel for one account. An account's card says "Signed in" only on a yes of Claude Code's own status check (or on a mark of the first test build); after a sign-in or a check without an answer it reads "Sign-in not confirmed". Skipper never reads or copies Claude credentials, and never removes or changes the sign-in variables you set, such as `ANTHROPIC_API_KEY`; a cloud credential is passed on only after you allow it by name.
- Code: type `Account`, module `accounts`
- Avoid: "login" as a noun, "profile", "user" (that is the person)

### Account mode
How roles are mapped to accounts. There are three modes: single-account, multi-account and mixed. In a single-account setup one account carries all roles; this is the intended default for new setups. In a multi-account setup each role, or group of roles, has its own account. A mixed setup does both, for example the lead on one account while two other roles share another. Sharing one account between several roles is untested and is only called supported after it has been tested.
- Code: enum `AccountMode`
- Avoid: "split account", "account pool", "account switching" (in the current test build a role runs on the account it is given; continuing a role on another of your own accounts at a limit is in development)

### Account shared with the person
An account that the person works on too, outside Skipper. Skipper never pushes such an account: no fill-in offer, no idle card, no start request, no fact to the Lead that hands out work; the holds for the limit, the end of work and the weekly gate stay. It is shared when the person marks it by hand, or as soon as Skipper sees use outside Skipper on it (it then stays shared to the end of that weekly window, in every project and across a restart of Skipper: the PC remembers it in `state\outside.json`). The person's word goes only one way: a mark that is taken back never clears what Skipper saw. Not to be confused with a shared account, which is used by more than one role.
- Code: `SharedBy`, `Effective::shared`, `shared` in `[accounts.<id>]` of `pacing.toml`
- Avoid: "locked account", "protected account"

### Append-only
A property of the board: entries are only ever added, never edited or deleted afterwards. A correction is a new entry that refers to the old one, so the history stays readable.
- Code: log type `EventLog`
- Avoid: "edit entry", "rewrite history"

### Approval
A decision that a piece of work may move on to the next approval level. Depending on the level it is given by a role that holds the right for that level, such as the lead, or by a person; the final approval is always given by a person. It is not the same as a permission prompt of Claude Code: in 1.0 every role starts with auto mode switched off, so such a prompt waits for you, in the terminal or on a permission card, and no lock rests on your answer.
- Code: type `Approval`
- Avoid: "permission" (see Permissions table), "release", "merge right"

### Approval ladder *(partly built)*
The ordered approval levels that work passes through. You define the levels yourself; a common setup is role branch, then an integration branch, then the main branch. Each level shows its evidence. A lead can be allowed to approve every level before the last one; it holds that right only if the permissions table gives it. The last level is always the final approval and is given only by a person.
- Code: types `ApprovalLadder`, `ApprovalLevel`
- Avoid: "stage" (reserved for the roadmap), "pipeline"

### Attention inbox
The place in the interface that collects everything waiting for a person: questions addressed to the person, approvals only a person can give, permission dialogs waiting in a terminal, accounts at their limit, and a lead that has dropped out. The list of decisions reserved for the person is configurable. The inbox never answers anything itself; a click jumps to the right panel or board entry.
- Code: module `inbox`
- Avoid: "notification centre", "alerts"

### Autopilot
Skipper's own mechanical work on the board, done by code and never by AI: it opens a role that has work, closes a role that has rested, recycles an idle role that has work into a new session, and raises one inbox item when the Lead has news. It runs only inside a team run and only while its own person-only switch is on; it works even when no Lead runs, and it never types into a session and never wakes an idle one. Person comes first, then the Lead, then the autopilot (design 8.11, MC-385).
- Code: switch `autopilot`, module `steer::autopilot`
- Avoid: "automation", "agent"

### Board
The shared message board through which roles and the person exchange tasks, reports, questions and notes. It is append-only, kept as an event log in plain text, and the core is its only writer. States such as "open" or "done" are always computed from its entries, never stored separately.
- Code: module `board`
- Avoid: "pinboard", "chat", "channel", "queue" (see Queue for a role's own list), and "board" for the task board (see Task board)

### Board entry
One record on the board. Entry types are task, start, report, objection, closure, question, proposal, note and seal. Every entry has a number, a time, a sender and a recipient (a role, everyone, or the person).
- Code: type `Entry`, enum `EntryKind` with variants `Task`, `Start`, `Report`, `Objection`, `Closure`, `Question`, `Proposal`, `Note`, `Seal`
- Avoid: "message", "post", "ticket", "event" in user-facing text

### Box job
The unit of slow work that the core hands to the team runtime: it is started without waiting, runs on a thread of the runtime in a role's box and reports each result back to the core as one command from the program. It prepares folders and git, starts the person's program in a check box and collects its output. The core writes every file and record itself; nothing slow runs on the command thread (design 12.12, MC-403).
- Code: `TeamControl::start_job`, command `JobFinished`
- Avoid: "background task", "thread" (a role never starts one)

### Briefing page
The page of everything since the person last looked: what was done, what runs, what comes next and how many items need the person. It is status only: it answers, approves and starts nothing. A fact list that the core computes from the board is its first layer; an optional summary written by a briefing session is its second. It is not the start briefing that a role receives in its first `whats_new`.
- Code: module `catchup`
- Avoid: "dashboard", "inbox" (that is the Attention inbox), "start briefing" for this page

### Briefing session
A fresh, short Claude Code session that Skipper starts, on the person's switch "Live summary" and only inside a team run, to write the optional summary of the Briefing page. It is a service terminal: the person's own unmodified Claude Code in a visible terminal, with a fixed prompt, a small model and two tools of its own, ended after its one job. Nothing it writes reaches a role.
- Code: service kind `Briefing`
- Avoid: "agent", "background job"

### Chain run
A run of one check set on the merges that the integrating role made, in that role's box. When the newest merge fails, Skipper searches the chain by halving and names the first merge that breaks the set and the task that merge belongs to, on the page `Integration: devops`. Skipper merges nothing, closes nothing and moves no approval level; what follows is the integrating role's decision (design 12.12, MC-412).
- Code: function `checks::chain` and the field `chain` of a `Checked` record
- Avoid: "bisect" (a git command that Skipper does not use), "auto-merge"

### Check run
One start of a check set's program in a role's box, on a working tree, on a reported commit in a fresh copy, or on a step of a chain. It has a number, ends as passed, failed, stopped at its time limit, not started or ended with its role, and leaves one `Checked` record and a digest. Skipper starts it only because the person named the command in a check set (design 12.12, MC-402).
- Code: type `RunId`, record `Checked`
- Avoid: "build" (a run need not build anything), "job" (that is the box job that carries it)

### Check set
One `[[checks]]` table of `workspace.toml` in which the person names a program, its arguments and how its output is read (`cargo`, `tap`, `junit` or `generic`), with a time limit, a network switch and the folders the program may read. It is the only way a program is started by Skipper: never from role text, a page or a task. A changed set is in force only after the person's Apply (design 12.12, MC-401).
- Code: type `CheckSet`, module `defs::checks`
- Avoid: "test suite" (a set can run any command), "pipeline", "script" (a batch or script file is refused)

### Circuit breaker *(not built)*
A safeguard in a flow: when a step has failed as often as allowed (three times by default), it is blocked and escalated instead of being retried again.
- Code: type `CircuitBreaker`
- Avoid: "kill switch" (see Emergency brake), "retry loop"

### Claim
The duties that a role definition lists as its own. The roles in a team that claim a duty decide, with the person's pins and the fallback chain, who owns it.
- Code: field `duties` of the role definition
- Avoid: "claim" for starting a task (that is a task claim, see Claim bundle)

### Claim bundle
What a role gets in the reply when it claims a task: the task brief (title, owner, done criteria in the order a check marks them, dependencies and their states, the description, and the last check when the task comes back from rework), the pages that name or are linked from the task, similar tasks with their review packs, how to report, and a suggested branch name. The clone exists since the role was made; at the claim Skipper creates the branch ref in the claimer's own clone and never switches to it (design MC-414), and it announces the page "Code map SK-12". The bundle is computed from the board and the knowledge base and writes nothing beyond the claim and that branch ref.
- Code: module `prep::bundle`
- Avoid: "assignment package", "ticket"

### Code map
The Skipper page `Code map SK-12` that Skipper writes when a role claims a task: the files of the claimer's clone that fit the task's words and the paths its description names, with the symbols found in them. Fixed patterns of `git grep` find the symbols, not a parser, so a name can be missing. Symbol names are role-written text and are quoted (design 12.12, MC-415).
- Code: module `prep::map`
- Avoid: "index", "symbol database"

### Command palette
A search box opened with the keyboard that offers every action: start a role, start a flow, switch layout, write a board entry.
- Code: module `palette`
- Avoid: "command bar", "launcher"

### Config directory
The folder where Claude Code keeps the sign-in and settings of one account. It belongs to exactly one location. Roles that share an account at one location use the same config directory. The same subscription used at two locations means two config directories and two sign-ins; credentials are never copied between them.
- Code: type `ConfigDir`
- Avoid: "profile folder", "credentials folder", "token directory"

### Connection
An MCP server that a role may use in addition to Skipper's own tool server, for example GitHub, a documentation lookup, a browser or an outside issue tracker. Skipper ships none and installs no runtime for them; it only keeps the list, decides which role gets which, and shows every connection on the role card, with labels such as "through Skipper" or "direct". Skipper keeps the tokens of a connection only in its vault (see Vault): the person signs in once in Skipper's own window, and a connection through Skipper runs without the role ever seeing a secret. The list starts empty, also with the starter team.
- Code: type `Connection`, file `connections.toml`
- Avoid: "plugin" (that is a Claude Code extension), "connector" (what Claude's website calls an account-wide service), "integration"

### Connection through Skipper
A connection that runs through Skipper instead of in the role's own box. The role's MCP file holds only a small relay entry for it (the relay mode of `skipper-mcp`, not the container relay of the glossary entry Relay); the sign-in helper makes the calls, inserts the saved sign-in at the bound address and hands back the result, which the role reads as data, not as an instruction. The role never sees a password, token or key. It is the default for every connection that is remote or needs a sign-in, a header, a token or an environment value; the other kind, `direct`, is a local program without any secret that runs in the role's box. Every call passes the core's decision: grant, known tool, protection tier, lock, sign-in, rule, review, first use and, where the rule says so, an ask that only the person answers in Skipper's window.
- Code: field `via` of `Connection`, module `hub`
- Avoid: "proxy" (Skipper puts none into the path of `claude`), "connector" (what Claude's website calls an account-wide service), "tunnel"

### connections.toml
The definition file in the workspace folder that lists the connections of one workspace: for each, its name, kind (a local program with arguments, or a remote URL), program folder, the names of the environment variables it needs, its tool rules, and the roles that may use it. It holds names only and never a value of a token or password.
- Code: file `.skipper/connections.toml`
- Avoid: "secrets file", "env file"

### Context limit
The point at which Claude Code compacts a conversation. It is set per model profile and per role, and Skipper writes it into the role's own settings file at start, never into an account's files. A change reaches a running role at its next resume, because Skipper never types into a session. Whether Claude Code accepts such a value per session is checked by a probe; until then the field may be greyed out instead of faked.
- Code: field `context_limit` of `Role` and `ModelProfile`
- Avoid: "memory" (that is RAM, or Claude Code's own memory feature), "token budget"

### Contract page
A knowledge page under `contracts/<name>` that two to four roles own together, for example the interface between backend and frontend. Only its owners write it and every role reads it; each saved revision makes the core ask every other owner a question to confirm or object, and the Lead settles an objection. A contract is how roles agree on something without a chat.
- Code: module `core::talk`, pages titled `contracts/<name>`
- Avoid: "agreement", "spec page", "shared doc"

### Core
The Rust program at the heart of Skipper. It starts and owns every role's terminal, is the only writer of the board, and works out states from events and hook signals.
- Code: crate `skipper-core` (not `core`, which collides with Rust's built-in crate of that name)
- Avoid: "engine", "server" (Skipper has no server), "daemon"

### Criterion check
A machine check that a done criterion may carry, written in one of five forms: `set <name>`, `test <set> <test name>`, `path <path>`, `no-path <path>` or `scope`. Skipper pre-marks the criterion from the check runs on the task's last reported commit (met, not met or unknown) as evidence for the checking role. The checker still marks every criterion; a disagreement is shown and never refused (design 12.12, MC-410, MC-411).
- Code: field `criteria_checks` of a task, module `checks::criteria`
- Avoid: "test case" (a check can also look at a path or the file scope), "automatic approval"

### Damping
Rules that keep the board from flooding the lead with wake-ups. Notes to the lead are bundled, and only reports, questions, objections and urgent entries are delivered at once. Repeated identical messages are dropped, and the number of waiting deliveries per role is capped.
- Code: type `Damping`
- Avoid: "throttling" and "rate limiting" (Skipper never limits usage, see Pacing)

### Deadline
The end-of-work time that the person gives, once ("until HH:MM, only today") or every day. It counts for an account only inside the 5-hour window it lies in: the pacing line then ends at the deadline, and afterwards everything is normal again. One shared deadline is read by every account that uses it.
- Code: table `[deadline]` of `pacing.toml`
- Avoid: "reset" (that is the end of an account's usage window), "shutdown"

### Delivery
Getting new board entries to a role. While its session is running, a hook reply announces new entries by count and number only; their text arrives through a tool call that the role makes itself, inside the sender frame (design MC-162). Skipper never types into a session: an idle role is reached when a person clicks into its panel.
- Code: type `Delivery`
- Avoid: "push", "notification", "injection", "typed notice"

### Destination binding
The rule that a saved sign-in is bound, when it is made, to one address (scheme, host and port) and one place in the request, and that the sign-in helper sends it only there, over HTTPS, and follows no redirect. The binding is sealed inside the saved sign-in itself, so an edited workspace file cannot send a secret elsewhere; when a connection's address changes, its sign-in stops until the person rebinds it or signs in again.
- Code: type `Binding` in `skipper-gateway`, `Origin` in module `hub`
- Avoid: "allow-list of hosts", "scope"

### Difficulty tier
How hard a task is: easy, normal, hard or frontier. The Lead judges it when it creates or assigns the task, rules adjust it (for example one more for security or data-loss work), and a role may report a task as harder than it looked. Skipper uses it, with the kind of work, to choose the model and effort of the session that does the task. Not a sandbox tier.
- Code: type `Tier` of `profile`, label on the task
- Avoid: "priority" (that orders tasks), "level", "tier" without "difficulty" when sandboxes are meant

### Digest
The short text that Skipper makes of a check run's output: at most 7,000 characters with the run, the set, the verdict, the command as the person wrote it, the place, the counts and the failed tests with file and line, inside a frame that says it is data from the project's own command. Control characters, secret-looking strings and frame-like lines are removed from every line, and the same output always gives the same text. A role reads the digest and never the raw output, which stays with the person (design 12.12, MC-406).
- Code: module `checks::digest`
- Avoid: "log", "output" (the raw output is not given to roles), "summary"

### Duties part
The text that Skipper writes for one role from the resolved owners of the duties: what is yours, who owns what is not, and which rights are missing. It is written by a pure function of the team definition, reaches a role at its start and, live, through a hook, and holds only Skipper's own catalogue text and role ids.
- Code: module `duties`
- Avoid: "system prompt" (it is one part of a role's start text), "rules"

### Duty
A kind of work in the team, such as tests or security. Stage 1 has a fixed catalogue of twelve duties. Each has a short description of what its owner does, the Skipper skills that go with it and the rights a holder needs.
- Code: module `duties`
- Avoid: "task" (see Task), "responsibility", "skill" (a skill belongs to a kit)

### Emergency brake *(not built)*
If the connection between the app and a container's relay is lost, for example after a crash, the relay interrupts every role in that container that is currently working and marks it stopped. This way nothing keeps running unseen.
- Code: module `relay`, function `emergency_brake`
- Avoid: "kill switch", "panic button"

### End hard
Ends a role's job object at once, without any key press, for example while a dialog is open in its terminal. The lead may do it directly for the roles in its list; like every action it is logged and shown.
- Code: command `EndHard`
- Avoid: "kill", "force quit"

### Envelope
The ceiling, set once by the person per project, for roles that the Lead may define without asking: the allowed catalogue bases, the widest file scope (by default the role's own clone only), the connections (none by default), the account and the number of helpers. A role inside the envelope is created without a card; anything beyond it is a card with a difference. The profile envelope is the same kind of ceiling for the model and effort that the Lead may ask for; it is made from the person's own model and effort bounds (design 8.11 and 12.13.7, MC-396, MC-397, MC-515, MC-529).
- Code: `[project.envelope]` in `features.toml`; the profile envelope is made by `steer::envelope::envelope_from` from the person's bounds (`[project.profiles]` of `features.toml` is still read but is no longer the source)
- Avoid: "sandbox", "policy"

### Escalation ladder
The fixed order of steps that the next session of a task climbs after a failed one: ultrathink on a new session, one more effort step, the next model up, then maximum effort or ultracode for hard work. It is per task and never sticky: the next task starts again from the start table. Rungs that bounds or quota do not allow are skipped, and after the last rung the Lead is told the task is stuck.
- Code: module `profile`
- Avoid: "retry loop", "boost"

### Evidence
The proof attached to a report or closure: a commit, a file, or a test command together with its result. Placeholders are rejected. The core checks evidence lightly and read-only; it does not re-run tests, and where it cannot check, it says "checked for form only".
- Code: type `Evidence`
- Avoid: "proof", "attachment", "artifact"

### Fallback chain
The ordered steps that find the owner of a duty when no role in the team claims it: for example the holder of another duty, each holder for its own work, each role in the team, the lead, or a named role. The person can change a duty's chain on the role page.
- Code: chain steps in `.skipper/duties.toml`
- Avoid: "failover", "retry"

### File scope
The set of files a role may change. For example, it keeps the lead from writing product code. It is enforced by a hook template, so it is a rule in Claude Code's hooks and not a lock in the operating system, and the documentation says so.
- Code: type `FileScope`
- Avoid: "sandbox" (see Sandbox level), "jail", "allowlist" for this meaning

### Fill-in work
Tasks that carry the label `fill-in`. When pacing finds an account behind its line, it may offer such a task to a role on that account that already owns it; Skipper never invents one, and the offer stays with that account.
- Code: label `fill-in`
- Avoid: "reserve work" in code (the operating-system reserve of the caps is `Reserve`)

### Final approval
The approval given at the last approval level, usually the main branch. Only a person can give it, with a plain-language confirmation in Skipper's own interface. The lead cannot give it, and board commands have no way to give it. This protects against roles and the programs they run, not against someone who controls the whole machine or a process that remote-controls the interface, and the documentation says so.
- Code: type `FinalApproval`
- Avoid: "gate", "auto-merge", "manual override"

### Flow *(not built)*
A template that you define: a sequence of steps across roles, stored as a plain text file. Skipper ships no ready-made flows, except one starter flow that comes with the starter team. It calls them flows, not workflows, to avoid confusion with workflows inside a single Claude Code session.
- Code: module `flows`, type `Flow`
- Avoid: "workflow", "pipeline", "playbook", "automation"

### Flow run *(not built)*
One execution of a flow with its parameters filled in. Its progress is computed from the board, so there is no second source of truth.
- Code: type `FlowRun`
- Avoid: "job", "instance"

### Flow step *(not built)*
One unit of a flow. A step has a name, an owner (a role, or "lead chooses"), a kind (task, check, human approval, wait, or script), inputs, expected evidence, limits for attempts and time, and an escalation path.
- Code: type `Step`, enum `StepKind`
- Avoid: "stage", "node", "action"

### Focus
One order of the Lead that closes the roles outside a named set and opens the roles inside it, each close saved first and each open under the guards. Closed roles keep their notes and their sessions, so a later focus on the earlier set resumes it. With "strict" nothing else runs; otherwise other accounts may do background fill-in work (design 8.11, MC-376).
- Code: `team_steer` action `focus`, record `FocusPlan`
- Avoid: "mode", "mute"

### Free role
A role that waits while work it may take up waits too: its turn has ended, it is not paused, no helper of it runs, and for a task it may claim, a task it holds or (for a role that checks results) a result that waits for its check. After a setting's time Skipper shows one card in Needs you and one line in the lead's `status`; it never types, wakes or starts anything (MC-325).
- Code: finding `FreeWithWork`
- Avoid: "idle role" (a role without work is not meant)

### Helper
A sub-agent that a role's own Claude Code starts inside the role's session. A helper has no terminal of its own, stays one level deep, and has the role's tools and never more. Its type and last sentence appear in a helper row.
- Code: record `HelperRun`
- Avoid: "sub-role session" (a separate session, see Sub-role session), "worker"

### Helper profile
A template for the sub-agents that a role starts by itself inside its own session: model, effort, task pattern and report length. Skipper manages the profile files and shows running helpers, but does not start them. It is one of the two kinds of sub-role; see Sub-role session for the other.
- Code: type `HelperProfile`
- Avoid: "sub-role" on its own, "worker"

### Helper row
One line per helper that a role started: its type while it runs and, once it ended, its last sentence as text written by the helper. Start and end are records of the board's log.
- Code: records `HelperStarted` and `HelperEnded`
- Avoid: "sub-agent log"

### Hierarchy bar
The strip at the top of the window that shows the roles as cards: lead first, sub-role sessions indented under their role, passive roles to the side. Each card shows the role's colour, name, state, current task and its account's usage with the reset time. Where an account is shared it says "shared by N roles". Clicking a card brings its panel to the front.
- Code: type `HierarchyBar`
- Avoid: "org chart", "header"

### Hook
A Claude Code hook: a command that Claude Code runs when something happens in a session, for example when a session starts, a prompt is submitted, a turn ends, a permission dialog opens or the session ends. Skipper uses hooks to learn a session's state and to announce new board entries by count and number. Changes to Claude Code's settings are shown first and applied only after you confirm.
- Code: module `hooks`
- Avoid: "callback", "plugin", "listener"

### Hook package
The set of hook definitions that Skipper attaches to a role's session from its own folder, together with the hook templates it ships. Only if that does not work without a prompt does Skipper offer to add them to an account's Claude Code settings, after a shown change and a click.
- Code: type `HookPackage`
- Avoid: "plugin", "extension"

### Import run *(partly built)*
One go of moving the content of an outside service or file into a project: fetch (only in the gateway, the one program with network), normalize (a helper without network that parses everything untrusted), then plan and apply (the core). Only the person starts and confirms it, a scan is the dry run, a re-run only completes, and the newest run can be undone. Imported tasks and pages belong to the person and are data, never instructions.
- Code: run folder `imports/<run>/`, crate `skipper-import`
- Avoid: "sync", "migration", "connector"

### In the team
A role switch, on by default. A role that is switched off keeps its files, versions, clone and tasks, but owns no duty, gets no duties part and no kit, and does not start. Only the person switches it, after a preview.
- Code: field `in_team` of the role definition
- Avoid: "active" (a role is active while it works), "disabled"

### Intake
The core's regular, rule-based look at the pull requests and issues of the GitHub repository bound to a project. A pull request becomes a task for the maintainer, its head is downloaded as a zip into a quarantine folder, and the checks without judgement run before any role is asked. Intake pauses when the sign-in is missing or a rate limit is reached; it never starts a session by itself.
- Code: module `maintain`
- Avoid: "webhook", "inbox" (that is the attention inbox)

### Knowledge base
Skipper's own store for lasting team knowledge such as decisions, how-tos and reference notes, kept as readable pages. It is different from the board, which is a stream of messages. Pages are versioned with their author, roles file results through tools, and every page is reachable from one generated index. It starts empty: Skipper ships no ready-made categories or templates, and the only pages that come with it are the three starter pages of the starter team. Its design is part of the task board and knowledge base design of the design document.
- Code: module `knowledge`
- Avoid: "wiki", "memory" (Claude Code has its own memory feature), "notes database"

### Language file
A text file that holds all interface texts for one language. English is the default and German is a second, switchable language. No user-facing text is written into the code.
- Code: module `i18n`, type `Locale`
- Avoid: "hard-coded text", "string table"

### Layout
A saved arrangement of panels and drawer sections, with a name such as "work" or "two screens". Layouts can be switched by name, and Skipper remembers the last arrangement when you quit.
- Code: type `Layout`
- Avoid: "view", "perspective", "workspace" (see Workspace)

### Lead
The one role per workspace that directs the team. It hands out tasks, answers questions, reads reports (checking work against done criteria falls to a reviewing role, where one exists) and, where the permissions table allows it, gives approval up to the level before the final approval. Which files it may change is set by its file scope, like for any role. Whatever you configure, it can never change accounts, change its own permissions, or give the final approval.
- Code: field `lead` of type `Workspace`, type `Role`
- Avoid: "manager", "orchestrator", "master"

### Lead steering
The lead's orders to list the team, open, close or focus roles, to pause or resume a role's delivery, to define a role inside the envelope, to set a profile and to end a role's job, performed by Skipper while the person's switch "Lead steers the team" is on and only inside a team run (design 8.11). It never types, never wakes an idle session, never changes an account and never answers a dialog.
- Code: switch `lead_steers`
- Avoid: "automation" (Skipper's own mechanical work is the Autopilot)

### Lesson *(not built)*
A knowledge page that records what was learned from a stall or from a finished run. The "record lesson" action pre-links it to the flow version, step, run and task numbers; it is found by search and shown as a backlink on the tasks it links. No template ships and tags are free.
- Code: a knowledge page with links, created by the action `record_lesson`
- Avoid: "post-mortem", "incident report"

### Line
The straight line of pacing, from 0 % at an account's window start to the target (99 % by default) at the end of its usage window: one minute before the reset, or at the person's deadline. The account's used share is judged against it.
- Code: module `pacing`
- Avoid: "quota", "budget"

### Location
Where a role's terminal and commands run: the host (Windows itself), which is the only location built so far. WSL, only where it is already present, and Docker, an optional backend for people who already have it, are planned and not built. A location can start a terminal, run commands, read allowed files and store files in the app's own folder there.
- Code: enum `Location`
- Avoid: "environment", "target", "machine"

### Machine arbiter
The one part of the app that every start of every project asks first. It counts the terminals of all open projects against one terminal cap and one operating-system reserve, applies each account's pacing gate, stagger and brakes, and orders waiting starts: the person first, then the Lead, then automatic starts, then services, and inside one kind "Priority", task priority, waiting time and a round robin between projects. A project never owns a share of the caps. Roles and the Lead see other projects only as counts, never by name.
- Code: module `machine::arbiter`
- Avoid: "scheduler", "queue manager", "load balancer"

### Maintainer
The fixed role that looks after the project's GitHub repository: it checks pull requests from other people and the team's own pushes, asks the roles a change concerns before it decides, and writes a verdict with its reason. It never merges or posts by itself: the person's one click is the final approval, and Skipper then merges pinned to the reviewed commit. It rests until it has work and has no git network.
- Code: role id `maintainer`, tool `pr_verdict`
- Avoid: "gatekeeper", "release manager", "bot"

### Model profile
The default effort and the context limit for one model, for example `opus` or `sonnet`. A role takes its model, effort and context limit from the profile or sets its own value, and a helper profile can do the same; the most specific layer wins. The list of profiles is data, so a new model needs no new Skipper release.
- Code: type `ModelProfile`, file `models.toml`
- Avoid: "preset", "model preset"

### Next work
What the reply to a role's report, question or (for a role that checks results) closure names: the next task it may start, the result that waits for its check, the task it still holds, or why none can start. It is text in the reply to the role's own call; claiming stays the role's act (MC-326).
- Code: function `next::reply_tail`
- Avoid: "assignment" (nothing is assigned by the reply); for a role's own list of open tasks see Queue

### Objection
A board entry that blocks a piece of work and states why. Only its author can close it. If the parties disagree, the lead decides.
- Code: `EntryKind::Objection`
- Avoid: "veto"

### Open project
A project whose core is loaded in the app, so the rail lists it with its counts. Opening never starts a session; a project is running only when it also has a team run. Several projects can be open at the same time, and one of them is the shown project.
- Code: one world per open project in `worlds`
- Avoid: "active project" (that no longer names one project), "loaded workspace"

### Order
One request of the Lead or of Skipper to open, close, focus or profile a role, with an order id. A role has at most one live order; every order ends in an outcome (started, closed, held, lapsed, failed, interrupted, cancelled) that the Lead reads with `whats_new` (design 8.11, MC-386, MC-387).
- Code: record `Steering` with an `origin`, then `SteerOutcome`
- Avoid: "command" (that is a core command), "job"

### Owner
The single role responsible for a task. Every task has exactly one owner, and only the owner may start it or report on it. In the decision marks of the design, such as "(owner decision)", "owner" means the project owner instead. For the role that holds a duty see Owner (of a duty).
- Code: field `owner` of type `Entry`
- Avoid: "assignee", "co-owner"

### Owner (of a duty)
The result of resolving one duty: one role, "each holder for its own work", undecided, nobody, or not in this team. Exactly one value per duty; a role owner is always a role in the team. A duty that two roles claim without a pin stays undecided and gives no role its lines until the person decides.
- Code: values `Role`, `Each`, `Undecided`, `Nobody` and `NotInTeam`
- Avoid: "assignee", "task owner" (see Owner)

### Pacing
Per-account control of new starts so that the account's usage reaches its target at its reset or at the person's deadline. It can be switched on or off per account. It holds new starts only, never running work. It looks at one account at a time: pacing itself does not move work between accounts or switch an account, and it never starts a session. The used share with the reset time stays visible when pacing is off.
- Code: module `pacing`
- Avoid: "throttle", "quota manager", "rate limiter"

### Panel
The area of the window that holds one role's visible terminal. Panels can be arranged freely, split, stacked and detached into their own windows. A collapsed panel keeps a strip with the last terminal line, so a working role is never invisible. Closing a panel stops that role.
- Code: type `Panel`
- Avoid: "pane", "tile", "widget"

### Parked
A task in progress or rework whose owner has no running session. It keeps its owner and its mark, is never takeable by another role, and shows "paused, owner closed since hh:mm, saved #n". The Lead releases it with `task_assign` and a reason (design 8.11, MC-393).
- Code: derived flag `parked` on the task view
- Avoid: "stuck", "stale"

### Passive role
A role you mark as passive. It does not hand out tasks, acts only when the person or the lead calls on it, and is never brought to the front on its own. Any role except the lead can be marked passive.
- Code: field `passive` of type `Role`
- Avoid: "overseer", "supervisor", "observer"

### Pause
Interrupts a role's current turn, meaning the work Claude Code is doing for the latest prompt. The panel and the session stay open. Background helpers the role started keep running, and no new board entries are delivered until you continue. Pause is not the same as stop.
- Code: command `Pause`
- Avoid: "suspend", "freeze"

### Permission card
An item of the attention inbox for one of Claude Code's permission questions: the role, the tool and the exact command, path or address. The person's click on Allow or Deny is relayed through Claude Code's permission-request hook; Skipper never decides, never presses a key and never saves a rule. When a role has asked the same simple shell command several times, the card also offers "Always allow exactly this…": it answers nothing and only opens the role's settings with the command filled in, and the rule exists only after the person saves it (design 12.12.8, MC-417). The card itself exists only in memory while it is open.
- Code: type `CardView`; the button is `CardView::offer_always`, the list is `allow_commands` in the role file
- Avoid: "approval" (see Approval), "permission prompt" (that is Claude Code's own dialog)

### Permissions table
A table that says who may do what on the board. Rows are roles or groups of roles. Columns are entry type by recipient, plus switches such as closing other roles' tasks, setting a seal, controlling the team and giving approval. Every refusal names the right way to get the thing done. It does not replace Claude Code's own permission system: in 1.0 every role starts with auto mode switched off, so the permission prompts that Claude Code raises stay with the person.
- Code: type `Permissions`
- Avoid: "ACL", "privileges", "rights"

### Priority
The window shows it as "Priority" (in the German interface „Vorrang“), an entry in the options menu of a project in the project rail, shown with a tick while it is on. It puts the waiting starts of that project ahead of those of other projects of the same kind, when the machine arbiter has to choose. It orders only; it never starts, stops or skips a guard. It is a mark on a project, not the priority of a task.
- Code: field `first` of a project, set `first` in the arbiter
- Avoid: "boost", "favourite"

### Project rail
The list at the top left of the window with one line per open project: its name, a run dot (running, idle, stopped), the number of things that need you and the number of running terminals, and "New project…" at its end. One click, Ctrl+Shift+1…9 or Ctrl+Tab shows a project; nothing is stopped, paused or asked. It replaces the project selector.
- Code: `RailItem` and `RunDot` in the app model
- Avoid: "project switcher", "tabs", "selector"

### Protective box
The default, Windows-native separation of a role (tier 1): its own identity with only the folders it was given, and memory and core caps from the operating system. It needs no administrator rights, no restart and no Docker. It is weaker than a Linux container, because the kernel is shared, and Skipper says so on every role. A weaker fallback of it is labelled as weaker.
- Code: crate `skipper-confine` (proposal), tier `native-box`
- Avoid: "container" (it is not one), "jail"

### Provider
A coding tool that Skipper can run in a panel. Claude Code is the first and, for now, only provider. Support for other tools is a plan for later and not a promise. The place in the design where a provider plugs in is called the provider slot.
- Code: trait `Provider`
- Avoid: "driver", "engine", "model" (that is the AI model chosen for a role)

### Question (typed)
A board entry that one role addresses to another role with `ask_question`, either to clarify or to decide. An open question is work for the addressee: it is counted in the hook reply, listed in `whats_new` and ahead of tasks in the reply tail. It never types into or wakes a session, it is information and never the person's order, and the Lead sees only counts of the traffic.
- Code: `ask_question`, `needs = [role]`
- Avoid: "message", "chat", "DM"

### Queue
A role's own open tasks in the order it should take them. The next one is named in the reply to the role's report; claiming it stays the role's own act.
- Code: module `prep::next`
- Avoid: "assignment list", "board" (see Board)

### Relay *(not built)*
A small helper program from the same Rust project that Skipper starts inside an optional container backend such as Docker, so roles there can reach the app without opening a network port. It carries board commands, hook signals and status line data, and it triggers the emergency brake if the connection is lost.
- Code: module `relay`
- Avoid: "agent", "proxy", "daemon"

### Report
A board entry in which a task's owner says the work is done, with evidence attached.
- Code: `EntryKind::Report`
- Avoid: "status update", "result"

### Resume
Starting a role's Claude Code again in its earlier conversation, using the remembered session ID. This is the default on every start. A fresh start happens only when you ask for it and confirm, and Skipper never starts fresh by itself.
- Code: function `resume_session`
- Avoid: "restart", "reload", "continue" as a command name (it can pick up another role's conversation when roles share a folder)

### Review pack
A Skipper page, "Review pack SK-12" for the task SK-12, that Skipper writes after each recorded report of the task, one version per check round. It lists the commit and the scope check, the range of changed files with their line counts (names and counts only, never file content), paths outside the file scope, paths with a secrets-file name, what changed since the previous round, the files the report named, the role's own test evidence marked "Skipper did not run it", one line that names Skipper's own check runs on the commit (the page "Checks SK-12"), and the done criteria as an unmarked checklist. Skipper never closes a task; it only pre-marks the criteria that carry a check, as evidence, and the checker still marks every criterion.
- Code: module `prep::review`
- Avoid: "review report", "automatic review", "verdict"

### Role
A named member of the team. It has its own panel and runs one Claude Code session, and it points to exactly one account and one location. It also has instructions, a model, an effort level, a context limit, a file scope and permissions. You create and name roles freely, for example `lead`, `writer` and `reviewer`. Skipper's only ready-made roles are those of the optional starter team, which you can edit or delete.
- Code: type `Role`, module `roles`
- Avoid: "agent" (Claude Code uses it for sub-agents), "worker", "bot", "persona"

### Role kit
The Skipper skills that a role gets: base skills, base skills for roles that write code, and the skills of the duties it holds. It is a folder of plugins in the role's derived folder, passed with one `--plugin-dir` argument at every start and resume.
- Code: folder `kit\` in the role's derived folder
- Avoid: "package", "plugin" (a kit holds plugins), "toolbox"

### Role note
The short page that a role writes with `page_write` to `notes/<role>` before it ends a turn with unfinished work, or when it sees a wind-down: what is done, what comes next, open decisions. It is at most 4,000 characters, only the role itself writes it, and the newest one reaches the hand-over page of the role's next session (design 8.11, MC-389).
- Code: the knowledge page `notes/<role>` and the record `Checkpoint {kind: Role}`
- Avoid: "checkpoint" (that is the record, not the page), "memory"

### Running project
An open project with a team run. Any number of projects may run at the same time in one Skipper app; the machine caps and every account bind all of them together. Stopping one never touches another.
- Code: the start state of a project's world
- Avoid: "active project"

### Runs along
The window shows it as "Runs along" (in the German interface „Läuft mit“), an entry in the options menu of a project in the project rail, shown with a tick while it is on. "Start all" starts the Lead of every project that is ticked. The default is the projects that were running when Skipper was last closed; only the person ticks and unticks.
- Code: field `runs_with` of a project in `projects.toml`
- Avoid: "autostart" (nothing starts by itself with Windows or with the app)

### Safe close
Closing a role without a key and without a wake: a working role is told to finish its step and write its note, Skipper waits until the role is idle, ends the job, and takes a snapshot of the clone; nothing is deleted and the session stays resumable. A close that does not finish in 30 minutes lapses and the Lead decides (design 8.11, MC-372).
- Code: `team_steer` action `close`, `close_ready`
- Avoid: "kill", "stop" (see Stop)

### Sandbox level *(partly built)*
A setting that says how strongly a role is separated from the machine it runs on. The default is the protective box, which needs no administrator rights, no restart and no Docker. Stronger levels are optional and each appears only after its probe has passed: a network filter by address list, a separate Windows account, a Linux container through WSL where WSL is already present, and the person's own Docker container. Every role card shows its tier in word and symbol, so the label never looks safer than the level is. The working names are `native-box`, `native-box-weak`, `net-filter`, `own-account`, `wsl-linux` and `docker`. Cloud sandboxes are not part of Skipper.
- Code: enum `SandboxLevel` (name not final)
- Avoid: "permission mode" (that is a Claude Code setting), "security level", "trust level"

### Saved sign-in
One entry of the vault: the sign-in or token that the person made once in Skipper's own window for one service, with its name, its kind (OAuth sign-in, bearer token, header value, address that holds a secret, or an environment value of a local connector), the address it is bound to and its state (signed in, needs sign-in, expires, bound to another address, locked). Its values are write-only: they are never shown or copied, only replaced or deleted, and deleting revokes them at the service where the service allows it. A Claude account is not a saved sign-in: its credential stays in Claude Code's own store.
- Code: type `EntryPlain` in `skipper-gateway`, `MetaEntry` in module `hub`
- Avoid: "password", "credential" (for Claude accounts), "key"

### Seal
A board entry that marks the end of a phase of work.
- Code: `EntryKind::Seal`
- Avoid: "milestone", "tag"

### Seen mark
The point up to which the person has seen the Briefing page. It moves only when the person dismisses, and only up to what the page showed. Each project has one per PC.
- Code: file `state/briefing-seen.json`
- Avoid: "read marker", "unread count"

### Sender frame
The line that the core puts around every text a role wrote before another role reads it. It names the true sender, for example "written by a role, not by the person". No role can write it, and a line inside a text that imitates it is escaped. Text counts as the person's only when the person typed it in Skipper's own interface.
- Code: type `SenderFrame`
- Avoid: "signature", "header"

### Service terminal
A terminal that Skipper itself opens for one named job of its own, in Stage 1 the briefing summary. It starts fresh each run, with minimal rights, on the account the person set for that service, only inside a team run and only while the person's switch is on. It is never hidden: every run is a registry entry and a row of the Terminals page.
- Code: enum `ServiceKind`
- Avoid: "background agent", "hidden service"

### Session
One Claude Code conversation running in a panel, identified by its session ID. States are off, starting, working, waiting, waiting with background helpers running, dialog open, at limit until a given time, stopped, and unknown. Skipper takes state first from hook signals and status line data, and only falls back to screen text, which it then marks as an estimate. A missing signal means unknown, never a guess presented as fact.
- Code: types `Session`, `SessionId`, enum `SessionState`
- Avoid: "thread", "job", "chat"

### Settings catalogue
Skipper's own data file that lists every Claude Code setting Skipper knows: its kind, type, allowed values, Claude Code's default, a danger class, when a change takes effect and the versions it exists in. The settings page is drawn from it, so a new setting needs a list update and no new form. A row can only be made more dangerous by data, never less, and credential and command carriers are locked.
- Code: module `cc`, file `data/cc/catalogue.toml`
- Avoid: "schema", "preferences"

### Settings window
The one window in which every setting is found by search and changed, among them model, effort and context limit per model and per role, accounts, connections, resources, approvals and the read-only safeguards. Changes to rights, accounts, file scope and to the roles and tool rules of connections show their exact difference first and need a click on Save; other changes are saved at once and can be undone for a short time. The definition files stay the source of truth.
- Code: module `settings`
- Avoid: "preferences", "options", "config screen"

### Shared account
An account used by more than one role. All its roles draw on the same usage limit, so Skipper shows "shared by N roles". At one location they use one config directory, and only one panel signs in while the others wait. Roles share an account only within one person's setup; sign-in always uses your own subscription.
- Code: derived from `Account` with more than one `Role`
- Avoid: "split account", "pooled account", sharing an account between people

### Shown project
The one project whose panels fill the window right now. The other open projects keep running and keep reading their terminals; only their painting is skipped. Showing another project is not a switch: nothing stops and the account stays signed in.
- Code: `shown` beside `worlds` in the app
- Avoid: "current project", "active project"

### Side drawer
The area beside the panels with sections for the board, the task board, flows, the knowledge base (page tree, search and backlinks), accounts and a log of all commands. Sections can be shown or hidden, reordered, and detached into their own windows.
- Code: module `drawer`
- Avoid: "sidebar", "inspector"

### Sign-in helper (gateway)
The separate program `skipper-gateway.exe` next to `skipper.exe`. It is the only process that decrypts the vault's secrets and, with the update helper, the only Skipper program with network code, through Windows' own HTTP stack. It runs as the person outside every protective box, has no window of its own, makes the calls of connections through Skipper, inserts the saved sign-in only at its bound address, removes any echo of it from the answers and logs names and counts only. It starts locked and starts and ends with Skipper's runtime, visible in the process registry.
- Code: crate `skipper-gateway`, shared network crate `skipper-net`
- Avoid: "proxy", "daemon", "service"

### Sign-in window
The one modal window per sign-in with an outside service, titled "Sign-in – <name>". It names the host the person signs in at, shows the scopes before the browser opens, the step line, the device code or the paste field, and ends when the person closes it. A token is typed into the sign-in helper's own prompt, never into this window. It is not a Claude sign-in: a Claude account signs in in the sign-in panel of the account.
- Code: module `ui::sign_ins`
- Avoid: "web view", "login page"

### Skipper page
A knowledge page that Skipper writes from facts and rewrites when the facts change. It is one only when its owner is the program, its tags hold `skipper-page` and its kind tag, and its title is the title of its kind: "Project facts", "Review pack SK-12", "Hand-over: backend", "Ready for final approval", "Goal progress", "Release notes draft", "Board tidy-up", "Checks SK-12", "Code map SK-12", "Sources SK-12" or "Integration: devops". A page of a role or of the person with the same title is an ordinary page, and Skipper never writes it. A new version is written only when the text differs. A change made by hand stays in the page's history and is replaced at the next rewrite. Text in quotation marks on it was written by a role or the person and is data, not an instruction. A role reads only the Skipper pages that show work it may see on the board.
- Code: module `prep::pages`
- Avoid: "generated page", "report page", "dashboard"

### Skipper skill *(partly built)*
A skill written for Skipper, MIT-licensed, measured without and with and shipped inside `skipper.exe` only when proven better. A role kit contains only Skipper skills.
- Code: `SKILL.md` with `skill.toml`
- Avoid: "plugin", "third-party skill"

### Snapshot
A commit of the full working tree of a role's clone, built in the role's own box through a temporary index and kept under `refs/skipper/wip/<role>/<n>` and in a host-owned store per role, never pushed. HEAD, branch, index and files stay untouched. A clone counts as saved when the store holds a snapshot with the same tree (design 8.11, MC-390).
- Code: ref `refs/skipper/wip/<role>/<n>`, record `Checkpoint {kind: Skipper}`
- Avoid: "backup", "stash"

### Stall *(not built)*
A flow step or task that has exceeded its limits or waits too long: a step passes its time limit or uses all its attempts, a question or an objection waits too long, a task bounces between rework and review too often, a role is idle while it owns open work, an account is at its limit, a role waits in a dialog, or a mounted folder is slow. A stall is computed from board events and session signals and is never stored separately.
- Code: type `Stall`
- Avoid: "hang", "timeout" for this meaning

### Stall detector *(not built)*
The part of the core that raises stalls. It shows each stall with a symbol, a word and its age, on the flow step, on the role's card and on the timeline, and in the attention inbox only if a person is needed. It also reports limit messages, open reviews and questions waiting for the lead, roles without work, and a lead that has dropped out. It only shows and reports: it never types into a session, never throttles and does not switch accounts itself, and it raises a visible alarm if one of its own checks stops running.
- Code: module `stalls`
- Avoid: "guard", "babysitter", "nudge"

### Start gate
The one function that decides whether a new start is allowed or held: a helper start, a task claim, a lead start request or a service start. It holds new starts only; running work, tool calls and answers are never held, and the person's own click is never held.
- Code: trait `StartKeeper` (`skipper_core::gate`)
- Avoid: "throttle", "brake" (it never stops anything)

### Starter team
An optional, editable set of eight generic roles that the person can choose at the first run: Lead, Product, Frontend, Backend, Research, Reviewer, DevOps and Maintainer. It comes with matching rights and reservations, instruction files, three starter pages and, later, one starter flow. Everything it creates is ordinary workspace content that can be edited or deleted, and nothing in it is a lock. It creates no account, no task and no connection. The other choice at the first run is to start empty.
- Code: templates shipped as data files, module `starter`
- Avoid: "default roles", "built-in roles" (they are ordinary content, not built in)

### Status line
The Claude Code status line, a small display inside a Claude Code session. Skipper may offer a small status line snippet that sends usage, reset time and context size to the app. Like hooks, it is shown first and applied only after you confirm.
- Code: type `StatusLine`
- Avoid: "footer" (the app has its own footer bar for account usage)

### Stop
Ends a role's work cleanly. It interrupts the current turn, also stops the role's background helpers, and marks the role stopped. Closing a panel stops the role.
- Code: command `Stop`
- Avoid: "kill", "terminate", "halt"

### Sub-role session
A separate Claude Code session with its own panel that belongs to a role and reports to that role instead of to the lead, for example a session for checking results under a role named `writer`. It is one of the two kinds of sub-role; see Helper profile for the other.
- Code: type `Role` with a `parent` field
- Avoid: "sub-role" on its own, "sub-agent", "child role"

### Subscription
The paid plan that a person signs in with inside Claude Code. One subscription may appear as several accounts, one per location. The accounts view groups them as subscription, then locations, then roles.
- Code: grouping in module `accounts`
- Avoid: "plan", "license", "seat"

### Take
One board command by which a role with the right takes an open task that has no owner: the owner is set and the work started in the same step, the first caller wins and a later caller is told who took it. Labels `for:<role>` and `duty:<id>` limit who may take a task (design 8.11, MC-392).
- Code: `task_claim` on an ownerless Open task, refusal `board.already_taken`
- Avoid: "grab", "pull"

### Task
A board entry that hands work to one owner. Its state is computed from the board entries that refer to it: inbox (proposed by a role that may not hand out work), open, in progress, review, rework, done, deferred or cancelled.
- Code: `EntryKind::Task`, enum `TaskState`
- Avoid: "ticket", "issue" (reserved for the GitHub issues of this repository), "to-do"

### Task board
Skipper's own place to plan and follow tasks, shown as columns: a leftmost column for items that need the person, then inbox, open, in progress, review, rework and done, with deferred and cancelled as collapsible columns. It is a set of views (columns, a list and a page view) over the board's entries and is designed together with the knowledge base. It starts empty, with no ready-made tasks, labels or templates.
- Code: module `taskboard`
- Avoid: "board" on its own (that is the message board), "tracker"

### Team run
The period from the person's click on Start team until Stop all, closing the app, or the moment no role runs any more; showing another project does not close it. Pause all suspends it until Resume all. It exists only in memory and is never restored after a program start; lead steering, the autopilot and service terminals work only inside it. In a project that starts with the Lead first, Start opens the team run and starts only the Lead (design 8.11).
- Code: grant `TeamRunGrant`, in core memory only
- Avoid: "session", "autostart"

### Terminal
One Claude Code session in a pseudo-terminal that Skipper started and lists. There are three kinds: a role terminal, a sub-role session and a service terminal.
- Code: kinds `Role`, `SubRole` and `Service`
- Avoid: "console", "window" (see Panel)

### Terminal cap
The person's limit on how many terminals may run at once. It binds lead-steered starts and service starts; a start that the person clicks only gets a warning.
- Code: key `max_terminals` in `machine.toml`
- Avoid: "role limit"

### Theme
The colour scheme of the interface. A light and a dark theme exist, and the interface can also follow Windows' own setting, with a plain look: thin lines, small corners, colour used mainly for state.
- Code: enum `Theme` with `Light` and `Dark`
- Avoid: "skin", "mode"

### Tier *(partly built)*
A sandbox level in the numbered list of the design: T1 the protective box (the default), T1b its weaker fallback, T2 a network filter by address list, T3 a Windows account of its own, and T4 the Linux tier through WSL; Docker is an optional extra backend. Every role card shows its tier. Not to be confused with a roadmap stage or an approval level.
- Code: the variants of `SandboxLevel`
- Avoid: "stage" (reserved for the roadmap), "level" on its own (that is an approval level), "edition"

### Timeline *(not built)*
An optional view with one horizontal track per role, the lead on top. Marks show tasks received, reports, questions, objections and limits, so you can see at a glance who is stuck.
- Code: type `Timeline`
- Avoid: "history", "Gantt chart"

### Tool rule (allow / ask / deny)
The setting of one tool of a connection: allow, ask or deny. The default is ask, which shows a prompt in the role's own terminal that only the person answers. Skipper writes the rules as permission rules into the role's own settings file, never into an account's files. Tools that publish, merge, delete, pay, sign in, change visibility or settings, or write secrets or webhooks on an outside service are never allow by default: tools whose names contain words such as merge, push, delete or publish start at deny, and loosening such a rule needs a shown difference, a click on Save and a red warning, after which the connection carries a red label. For a connection that runs through Skipper the sign-in helper enforces the rule and the person answers an ask in Skipper's window. For a direct connection a tool rule is a rule inside Claude Code's permission system, not a lock in the operating system.
- Code: enum `ToolRule` with `Allow`, `Ask` and `Deny`
- Avoid: "permission mode" (that is a Claude Code setting), "grant", "whitelist"

### Vault
Skipper's own store of the saved sign-ins of outside services, in a folder only the person, SYSTEM and administrators can open: one file per entry, sealed with Windows' DPAPI, and a metadata file without any value. No role can reach it, it is never exported, backed up or synced, and on another PC the person signs in again. Only the sign-in helper decrypts it; it holds nothing for a Claude account.
- Code: module `hub`, folder `vault` of `AppData`
- Avoid: "keychain", "password manager", "wallet"

### Watch finding
A fact that Skipper's watch computes from the role states, the board and the clock: a task left without a result after its owner's session ended, a role that repeats a refused tool call or keeps ending turns with an error, a role that does not finish starting, a role that is silent for a long time, and the items of the board tidy-up. A finding is never stored and never acts: it shows as a card in Needs you, as a line in the reply of `status` for roles that see the whole board, and, for the tidy-up, on a Skipper page. It never types, wakes, starts, pauses or stops anything, and it goes away when the facts change.
- Code: module `prep::watch`
- Avoid: "alert", "alarm", "supervisor"

### Workspace
One team setup. Its name, locations, roles, the lead, defaults, flows and the permissions table are definition files in a workspace folder. By default Skipper makes that folder itself, named after the project, in the main folder `Skipper-Projects` in your user folder; you can choose a folder that exists already instead. The folder must not lie inside a project folder that a role may write to. Runtime data, namely the board and the knowledge base, lives in Skipper's own app data and never in a project folder. A new workspace is empty, or starts with the optional starter team; you add the rest yourself. The app remembers recently opened workspaces.
- Code: type `Workspace`
- In the window: **Project** (owner decision, 2026-10-25). Design documents keep "project folder" and "project repository" for the code repository, which the window calls the "code folder".
- Avoid: "profile", "layout"

### Wrap-up *(partly built)*
A routine for the end of a working day, started by one button. It asks all roles to finish and save cleanly, stops them, runs the steps you have configured, and remembers the layout so that tomorrow's start resumes every role.
- Code: module `wrapup`
- Avoid: "shutdown" (that is turning off the computer), "end of day" as a command name
