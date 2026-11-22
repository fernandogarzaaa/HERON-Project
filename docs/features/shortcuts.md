# Run your Shortcuts

Heron can run shortcuts you make in the Shortcuts app. Through them it can reach Reminders, Calendar, Notes, Home, Music and other apps on your Mac.

## What Heron may run

Read this first.

- Heron runs only the shortcuts you tick. Nothing is ticked at first, so nothing runs until you tick one.
- A shortcut can do far more than its name says. It can send messages, change files or fetch web pages. Tick only shortcuts you made or have read.
- In Ask and Auto mode, Heron asks before every run. The approval shows the shortcut's name and the text Heron will pass to it.
- The approval offers "once" only. There is no "always" for a shortcut, and no rule or earlier approval covers the next run.
- In Yolo mode, a shortcut runs without asking, unless the conversation is tainted. In a tainted conversation, Heron always asks.
- The text Heron passes is sent to the shortcut, and what the shortcut returns is sent to your model provider, like anything else in the conversation.

## Choose the shortcuts Heron may run

1. Open Heron's Settings, then Features, then Shortcuts.
2. Tick the shortcuts Heron may run.

The list comes from the Shortcuts app. If a shortcut is missing, check that it exists in Shortcuts and open the list again. If the list says Shortcuts is not available, the `shortcuts` command is missing from this Mac.

## Make an "Add Reminder" shortcut

This shortcut turns Heron's text into a reminder. Through iCloud, the reminder also reaches your phone.

1. In the Shortcuts app, make a new shortcut named "Add Reminder". Let it receive text as input.
2. Add the action "Add New Reminder", and set its title to Shortcut Input.

Then tick "Add Reminder" in Heron. Now you can say "remind me to call the dentist" and approve the run.

To give the reminder a time, add "Get Dates from Input" before step 2. Then set the reminder's alert to the date it finds. A message such as "call the dentist Friday at 10" then gets a reminder at that time.

## Make an "Add Calendar Event" shortcut

This shortcut turns Heron's text into an event.

1. In the Shortcuts app, make a new shortcut named "Add Calendar Event". Let it receive text as input.
2. Add the action "Get Dates from Input", which reads the date and time from that text. Then add the action "Add New Event". Set its title to Shortcut Input and its start date to those dates.

Then tick "Add Calendar Event" in Heron. Now you can say "add lunch with Sam on Tuesday at 1pm to my calendar" and approve the run.

The event's title is the whole text, date included. Edit it in Calendar if you want a shorter one.

Action names can differ a little between macOS versions. The Shortcuts app asks for its own permission the first time a shortcut uses Reminders or Calendar. Heron itself needs no macOS permission.

## How a run works

- Heron passes its text to the shortcut as a file. Without text, it passes nothing.
- Heron reads what the shortcut outputs. To send text back, end the shortcut with "Stop and Output".
- If the shortcut fails, Heron tells you its exit code and the start of its error.
- If a run takes too long, Heron stops it and says so. The limit is 110 seconds, or less when Heron's tool timeout is shorter.

## Why Heron asks before changes afterwards

What a shortcut returns is outside content. A shortcut can fetch a web page or read someone else's text, and that text could hold instructions aimed at Heron.

So a shortcut's output marks the conversation as tainted:

- Heron still reads the output and tells you what it says.
- Changes and sends in that conversation ask you first, for example adding a task or running another shortcut.

## Limits

- At most 20,000 characters of text passed to a shortcut.
- At most 20,000 characters read back from it.

## Turn it off

"Run your Shortcuts" is on by default, but runs nothing until you tick a shortcut. To stop one shortcut, untick it. To stop them all, turn off the "Run your Shortcuts" row in Settings, Features. To block the tool for good, set it to "never" in Heron's tool settings.
