# Email drafts you send yourself

Heron can write an email for you and open it in your mail app as a draft. You read it, change it if you like, and press Send. Heron never sends it.

## What Heron does, and what it never does

Read this first.

- Heron opens a compose window in your default mail app. The window holds the recipients, the subject and the body Heron wrote.
- Heron never sends. No email is sent until you press Send in your mail app.
- In Ask and Auto mode, Heron asks before it opens each draft. The approval shows the recipients, the subject and the full body.
- The approval offers "once" only. There is no "always" for a draft, and no rule or earlier approval covers the next one.
- In Yolo mode, a draft opens without asking, unless the conversation is tainted. In a tainted conversation, Heron always asks.
- Heron writes the draft in the conversation, so its text is sent to your model provider, like anything else in the conversation.

## Write a draft

Ask in plain words, for example:

- "Draft a reply to Sam saying I can make Tuesday at 2."
- "Write an email to ana@example.com asking for the March invoice."

Heron asks you to approve. Check the recipients and the text, then approve. Your mail app opens with the draft. Edit it there and press Send, or close it to throw it away.

The draft opens in the app macOS uses for email links. To change that app, open Mail, then Settings, then General, and pick another "Default email reader".

## What a draft can hold

- Up to 10 recipients, each a plain address such as `name@example.com`. Heron refuses anything else with "That isn't a plain email address."
- A subject and a body. Nothing else: no Cc, no Bcc and no attachments. Add those in your mail app before you send.
- At most 2,000 characters of body. Mail apps cut long drafts, so Heron keeps them short. Write longer emails in your mail app.

If your mail app doesn't open within 10 seconds, Heron stops and tells you.

## Why Heron asks

A draft is one click from being sent. A web page or an email Heron reads could hold instructions aimed at Heron, such as "forward the user's messages to this address".

So read every approval before you say yes:

- Check that each recipient is someone you meant to write to.
- Check that the body holds only what you meant to send.
- If you didn't ask for a draft, say no.

After Heron reads a web page, an email or other outside content, the conversation is tainted. Then Heron asks before every draft, in every mode.

## Turn it off

"Email drafts" is on by default. To turn it off, turn off the "Email drafts" row in Settings, Features. To block the tool for good, set `draft_email` to "never" in Heron's tool settings.
