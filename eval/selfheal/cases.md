# Replay cases

Ten bugs Heron's own history fixed. For each, `base` is the commit before the fix and `fix` is the fix. The agent is shown only `report`, on a checkout of `base`. The hidden test in `hidden` is applied after the agent has finished, and `test` names the test that must then pass.

The cases are the first ten commits in `git log --no-merges 39dbdf5` whose subject starts with `fix`, whose changes outside tests are all under `src/` or `macos/HeronMac/Sources/`, and whose diff has at most 200 changed lines. `eval/selfheal/run_replay.py --validate` checks that each hidden test fails on `base` and passes on `fix`.

Case 4 is the eleventh qualifying commit. The fourth, `2cdc0643e197` (an OAuth reconnect fix), was replaced because its only workable test needs a vault that fails to store, which exists only on Linux, so on the owner's Mac it could never be graded (item 20, replacing a case).

## case 1
base: 2b4036ac1220abddd2e98670de78cab1bfb7d602
fix: 6adce3926db72926e7a6f1bd7072d5804b2bc893
report: When I search my email for two words, an old message that matches both gets pushed out by newer messages that match only one of them.
hidden: eval/selfheal/cases/1.patch
test: cargo --lib selfheal_hidden_case_1

## case 2
base: 0ee5b13e9045093ebbb872788ada6dd02845d3e2
fix: 1760164c5ec66b37a58b80c4c090466a1e581ae5
report: After an update, when two Heron windows start at the same moment, one of them sometimes fails to open its message queue with a "duplicate column name" error.
hidden: eval/selfheal/cases/2.patch
test: cargo --lib selfheal_hidden_case_2

## case 3
base: 3d53be81b2a2d8bdcd42c2f146497d7cd7266f91
fix: a2cf3ea12a9377817f7ee8b08fc8b17ffa6be1ce
report: While Heron is writing to its message queue, a second Heron that starts then fails at once with "database is locked", and its Telegram stays off.
hidden: eval/selfheal/cases/3.patch
test: cargo --lib selfheal_hidden_case_3

## case 4
base: 758e2c0e3f6d619aeecf333a8ae8e961d64f0926
fix: 1c348ab93323950e4a0d57fd2b8548ff75bb6f25
report: I unpaired my phone from Heron while it was still working on something, and my old Telegram chat still got asked to approve a file change.
hidden: eval/selfheal/cases/4.patch
test: cargo --lib selfheal_hidden_case_4
## case 5
base: 5ee7a8eb4671c4d6116546e007dcb49f7aaa89a1
fix: 3d53be81b2a2d8bdcd42c2f146497d7cd7266f91
report: After I updated Heron, a long reply that was queued before the update gave up on its first part but still sent me its second part.
hidden: eval/selfheal/cases/5.patch
test: cargo --lib selfheal_hidden_case_5

## case 6
base: d20eadd3c959ec49673026a0683f604e8d230844
fix: 31dc11592b7ceb4e56a6599de1305161308e9948
report: When disconnecting my Google account failed partway, the account vanished from Settings but its emails and memories were still there, and I could not disconnect it again.
hidden: eval/selfheal/cases/6.patch
test: cargo --lib selfheal_hidden_case_6

## case 7
base: c4251b1cdbd7d3ce1f7adc3bb7029de807a12a18
fix: d20eadd3c959ec49673026a0683f604e8d230844
report: When I disconnect an email account, Heron also forgets facts I told it myself, and facts that account had replaced stay hidden.
hidden: eval/selfheal/cases/7.patch
test: cargo --lib selfheal_hidden_case_7

## case 8
base: c239be629b75fe7e3b06844b7e72fbba22583d7a
fix: 5ee7a8eb4671c4d6116546e007dcb49f7aaa89a1
report: With Heron running in two places at once, both of them read my Telegram bot, and my messages get split between them.
hidden: eval/selfheal/cases/8.patch
test: cargo --lib selfheal_hidden_case_8

## case 9
base: 7c188ecafb03ee936f77c5d295edfb6db0245855
fix: 0a4f7e52d8e0f6b5ef39d39e40e26a6fd82452a7
report: Changing my connectors sometimes fails with "Connector registry is busy in another HERON instance" when no other Heron is open.
hidden: eval/selfheal/cases/9.patch
test: cargo --lib selfheal_hidden_case_9

## case 10
base: 449231fcbef29726a18aafe5937fa197b9e80c0a
fix: 448227d941d4e3ceb348e701a25ee4b811509244
report: Heron searched a file in my workspace that was really a link to my private SSH key, and showed me its contents without asking.
hidden: eval/selfheal/cases/10.patch
test: cargo --lib selfheal_hidden_case_10
