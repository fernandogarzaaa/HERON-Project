Version: 1

# Accepted risks

Heron is a research preview. This page lists the known security gaps that Heron does not close
in this version. The owner reviewed each one, decided to ship with it, and signed it below.
Heron shows you this list before your first conversation, and again whenever it changes.

Each entry says what can go wrong, who it can affect, and where the engineering notes track it.

## C35-8: Phone turns inherit the desktop permission mode

**What can go wrong.** Messages you send to Heron from Telegram run with the same permission mode
as the desktop app. In Auto or Yolo mode, Heron runs the actions those messages ask for, including
shell commands, without asking first. Anyone who controls your paired Telegram account can do
the same. Ask mode still asks before every change. High-risk requests, and requests that follow
outside content such as a web page or an email, are approved only at the computer.

**Who is exposed.** Anyone who pairs a Telegram account and uses Auto or Yolo mode. The risk
grows if someone else gets into that Telegram account, or if your phone is lost while unlocked.
To avoid it, use Ask mode, or don't pair Telegram.

**Where it is tracked.** T12 in the
[security threat model](engineering/SECURITY_THREAT_MODEL.md#known-weaknesses-not-mitigated), and
C35-8 in the
[remediation matrix](engineering/REMEDIATION_MATRIX.md#accepted-risks).

Status: Accepted by the owner (fernandogarzaaa) on 2026-11-18.

## R-1: Working-folder content can be sent out after an injected read

**What can go wrong.** Heron treats files in your working folder as yours, so reading one does
not make Heron more careful. If one of those files contains hidden instructions, for example a
downloaded document or a project you cloned, the model may follow them. It could read other
files in the working folder and send their contents to a web address the instructions name,
without asking you. Reading files outside the working folder still asks you first, and so does
reading files that look like secrets, such as `.env` files and keys. This entry covers ordinary
working-folder files only.

**Who is exposed.** Anyone who puts files written by other people into the working folder and
asks Heron to read them. To reduce it, keep private files out of the working folder, and don't
ask Heron to read files you don't trust from a folder that also holds private ones.

**Where it is tracked.** R-1 in the
[security threat model](engineering/SECURITY_THREAT_MODEL.md#accepted-risks) and in the
[remediation matrix](engineering/REMEDIATION_MATRIX.md#accepted-risks).

Status: Accepted by the owner (fernandogarzaaa) on 2026-11-18.
