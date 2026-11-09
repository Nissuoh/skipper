# Governance

**Status as of 0.1.0-test.6 (2026-11-09):** Skipper is not released and contributions are not open yet (see [CONTRIBUTING.md](CONTRIBUTING.md) and the [README](README.md)). This page says who decides and what happens when that person is away. It is short on purpose.

## Who runs Skipper

Skipper is run by one person, the GitHub account [@Nissuoh](https://github.com/Nissuoh). That account owns the repository and is its only maintainer. Where the project's files say "the maintainers" or "a maintainer", they mean this one person until this page names others.

## Who decides what

- The design document marks every decision (rule 6 of [CONTRIBUTING.md](CONTRIBUTING.md)). An `(owner decision)` changes only when the owner changes it. A `(maintainer's call)` may be challenged by anyone in an issue, with a reason. An `(open – owner decides)` question is not settled yet. A `(left to the builder)` detail is chosen by whoever builds it.
- Where a probe on a real Windows machine contradicts a maintainer's call, the probe wins and the call is reopened.
- Once contributions are open, the maintainer reviews pull requests and merges or declines each one with a reason. The maintainer's own changes pass the same checks, and nothing is merged while a check is red. (maintainer's call)
- There is one Skipper. Once the source is public, the maintainer will use the same public source and the same releases as everyone else. There is no private, owner-only, paid or feature-gated build and no hidden switch. The source is uploaded in small reviewed pull requests; from then on there is no code that is not in this repository. (owner decision)

## Adding a maintainer

Only the owner can add a co-maintainer. A co-maintainer is someone with a record of reviewed contributions to this repository. The owner names the person on this page, with their rights, before giving them write access. An `(owner decision)` stays with the owner. (maintainer's call)

## Community spaces

The project has a Discord server and an X account, both owned by the maintainer. The only official links are the ones in the [README](README.md); a server, account or page that is not linked there is not the project. Moderators are appointed and removed by the maintainer, and only people with a record of constructive help in the community are asked. A Moderator may delete posts and remove or ban people who break the [Code of Conduct](CODE_OF_CONDUCT.md), and is bound by it like everyone else. A Moderator cannot change this repository, its releases or its rules. No one from the project, Moderators included, ever messages anyone to ask for a password, a sign-in code or a token. (maintainer's call)

## Account protection

The maintainer's GitHub account is protected by two-factor sign-in. If the account is ever thought to be compromised, the project announces it publicly in the README and on the official links, revokes the releases made since the suspected date, and does not publish a new one until access is secured again. (maintainer's call)

## When the maintainer is away

- Short absences are not announced. Replies are best effort at all times.
- For a pause of more than about four weeks, the status line at the top of the README says "paused" and gives the date the pause started. There is no deputy, so private reports are answered when the maintainer is back. (maintainer's call)
- If the maintainer stops working on Skipper for good, the plan is to archive the repository, which makes it read-only, with a note at the top of the README, unless a hand-over has been announced in advance as the next point describes. (maintainer's call)
- The repository, the Discord server and the X account are handed over to someone else only after a public announcement made in advance, as a note at the top of the README. Without such an announcement the repository is archived, and the Discord server and the X account are closed or left read-only with a pointer to the repository, and none of them is handed over silently. (owner decision, 2026-10-21)
- The MIT licence lets anyone continue the work in a fork. A changed build or fork uses its own name and logo (see [`docs/brand.md`](docs/brand.md#name-and-logo)), and only builds from this repository's Releases page are releases of this project (see "Where to get Skipper" in the README).

## Changes to this page

Changes are made by pull request with a reason. Changes to "Who runs Skipper" and "Adding a maintainer" need the owner.
