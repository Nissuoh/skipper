Open to invited collaborators only until `CONTRIBUTING.md` says open.

## What and why

<!-- One topic per pull request, and a small one: within the size limit of rule 4 of `docs/github-workflow.md`, and a larger change is split into several pull requests. Say what changes and why. Link the issue it comes from: "Closes #n", or "Refs #n" when it does only a part. -->

## Design reference

<!-- The part of the design this follows, for example "design document, section 12". If the design has to change first, change it in its own pull request. -->

## AI assistance

AI assistance used: yes/no

<!-- If yes: which tool, and what it did. You have read, understood and tested every line yourself (rule 13 of `docs/github-workflow.md`). -->

## Checklist

- [ ] I followed `docs/github-workflow.md`: the change comes from an issue, the branch is named `type/short-topic`, the checks are green and none of them was skipped or weakened.
- [ ] Everything is in English: code, comments, documentation, commit messages and this description.
- [ ] Everything is Rust: no Python, JavaScript, TypeScript, Node or shell-script file or tool (`CONTRIBUTING.md` rule 12).
- [ ] Every commit has a `Signed-off-by` line (`git commit -s`).
- [ ] This pull request has one topic and stays within the size limit (docs/github-workflow.md rule 4). A fix, a refactor and a reformat are not mixed.
- [ ] Where code exists, tests are added or updated. No lock test and no check tool is red.
- [ ] A new dependency has a reason and passes the supply-chain rules of the design (section 17).
- [ ] Nothing is copied from other projects: no code, text, images, interface wording, names or vocabulary. I know where everything in this pull request comes from, including text produced with a tool.
- [ ] No Claude or Anthropic name in a package, library, binary, release, branch, tag, annotated tag text, image or video file name, or product string, and no Claude or Anthropic logo, mascot or look-alike art in any image or fixture (`CONTRIBUTING.md` rule 11).
- [ ] If this pull request adds or changes a GitHub Actions workflow, it follows rule 10 of `CONTRIBUTING.md` and names the rule.
- [ ] The words of `docs/glossary.md` are used as defined. A missing word is proposed here.
- [ ] Design text uses the marks of `CONTRIBUTING.md`: (owner decision), (maintainer's call), (open – owner decides), (left to the builder).
- [ ] There are no credentials, tokens, session transcripts or personal data. Test data, screenshots and recordings come only from dummy sessions or an empty demo workspace.
- [ ] There is one Skipper: this change adds no private, owner-only, paid or feature-gated behaviour and no hidden switch, and claims no automatic multi-account management.

## Safeguards

This pull request must not weaken any of the four hard locks (design section 5):

1. No account is put at risk.
2. Nothing runs invisibly.
3. Nothing reaches the main branch or the internet without a person's yes.
4. The PC does not freeze.

It adds no flag, file, variable or template that turns one of them off.

- [ ] This pull request does not touch a safeguard.
- [ ] This pull request touches a safeguard. It says so below and gives the reason. The account-safety rules of design section 9.2 change only with the owner's approval, and every rule except the eight that enforce the first hard lock is relaxed or removed only after Anthropic's written answer (design section 9.3).

<!-- If you ticked the second box, say which safeguard and why. -->
