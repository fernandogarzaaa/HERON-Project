# Quick entry and "Ask Heron about this"

Quick entry lets you ask Heron something without switching to its window. You can also send it text you selected in another app.

## What gets stored and sent

Read this first.

- Selected text you send with "Ask Heron" is stored with your message in Heron's database, like anything you type.
- That text is sent to your model provider, like anything you type. It is no more private than typed text.
- Selected text can hold passwords, private notes or other people's messages. Check the chip before you press Return.
- Nothing is sent until you press Return.

## The panel

The panel floats above any app. It has one text field, the reply below it, and "Open in Heron" to continue in the main window.

You can open it in four ways:

- from the menu bar extra;
- from the Services menu, with "Ask Heron" (below);
- from Shortcuts, Spotlight or Siri, with "Ask Heron" (below);
- with a global hotkey, once you record one.

There is no default hotkey. The obvious choices are taken by macOS. Record one in Settings. The hotkey needs no macOS permission.

The panel continues your last conversation if you used it in the past 30 minutes. Otherwise it starts a new one.

## Ask about selected text

1. Select text in any app.
2. Choose the app's menu, then Services, then "Ask Heron".
3. The panel opens with the text attached as a chip, "From" and the app's name.
4. Type your question, or leave the field empty, and press Return.

## Ask from Shortcuts, Spotlight or Siri

"Ask Heron" is also an action in Shortcuts, Spotlight and Siri. It takes one piece of text.

- It opens the panel with that text attached as a chip, "From Shortcuts".
- It never sends on its own. You type your own words and press Return.

This action needs a build step that may not work without Xcode. If `macos/HeronMac/APP-INTENTS-CHECK.md` says "Status: deferred to v1.1", the action is not in this version. The panel and Services still work.

## Why Heron asks before changes afterwards

Heron treats attached text as outside content. It did not come from you, even though you chose to send it. A web page or an email in that text could hold instructions aimed at Heron.

So attaching text marks the conversation as tainted:

- Heron still reads the text and answers questions about it.
- Changes and sends in that conversation ask you first, for example adding a task or running a command.
- If "Explain approvals" is on, the chip in the conversation names the source app, for example "Heron has read text from Preview in this conversation".

Heron learns memories from the words you typed, never from the attached text.

## Limits

- At most 3 pieces of attached text per message.
- At most 20,000 characters each.
- The source app's name is shown as given if it uses only letters, digits, spaces, `.`, `_` and `-`, up to 64 characters. Otherwise Heron shows "an app".

## Turn it off

Quick entry is on by default. Turn it off in Settings, Features, on the "Quick entry" row.
