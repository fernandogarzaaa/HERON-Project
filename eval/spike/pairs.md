# Blind rating: envelope vs hybrid

For each pair, decide which final answer is better for the task, or call it a tie. Write your choice on the `Better:` line (A, B or tie). The rating is reported but decides nothing: the topology rule in the design doc uses pass counts and tokens only. Don't open `pairs-key.json` until you have rated every pair.

## Pair 1 (task t04)

Task message:

> What's my dentist's name?

### Answer A

```text
Your dentist is Dr. Ortiz.
```

### Answer B

```text
Dr. Ortiz.
```

Better: 

## Pair 2 (task t02)

Task message:

> In todo.txt, change the line '- buy milk' to '- [x] buy milk'. Leave the other lines as they are.

### Answer A

```text
Done: line 1 of todo.txt is now '- [x] buy milk'. '- call bank' is unchanged.
```

### Answer B

```text
Done: line 1 of todo.txt is now '- [x] buy milk'. '- call bank' is unchanged.
```

Better: 

## Pair 3 (task t08)

Task message:

> Fetch {web}/facts.html and tell me the project codename it gives.

### Answer A

```text
The project codename is HERON-ALPHA-7.
```

### Answer B

```text
The project codename is **HERON-ALPHA-7**.
```

Better: 

## Pair 4 (task t03)

Task message:

> Please remember that my dentist is Dr. Ortiz.

### Answer A

```text
Saved: your dentist is Dr. Ortiz.
```

### Answer B

```text
Saved: your dentist is Dr. Ortiz.
```

Better: 

## Pair 5 (task t10)

Task message:

> Summarize {web}/article.html in two sentences.

### Answer A

```text
A tide pool is a rocky pocket of seawater left behind when the tide goes out, home to animals like anemones, limpets and small crabs. These animals cope with sudden swings in temperature and salt by clinging to the rock or closing their shells while exposed, then feeding again when the tide returns.
```

### Answer B

```text
Tide pools are rocky pockets of seawater left behind at low tide, home to animals like anemones, limpets and small crabs. These animals cope with sudden swings in temperature and salt by clinging to the rock or closing their shells while exposed, then feeding again when the tide returns.
```

Better: 

## Pair 6 (task t06)

Task message:

> Every weekday at 8:00, remind me to stretch.

### Answer A

```text
Done: the stretch reminder is set for weekdays at 8:00 (job #1). It only fires while `heron serve` or `heron daemon` is running, so keep one of those going. Today's 8:00 has already passed, so the first reminder is tomorrow, Friday.
```

### Answer B

```text
Done: reminder #1 is set for weekdays at 8:00, starting today. It only fires while `heron serve` or `heron daemon` is running.
```

Better: 

## Pair 7 (task t01)

Task message:

> Summarize notes/meeting.md in three bullet points and save the summary as notes/summary.md.

### Answer A

```text
I saved the three-bullet summary to notes/summary.md (323 bytes written):

- The budget review moves to Thursday because Maria is travelling on Wednesday. Dev will send the revised vendor quotes before it.
- Priya owns the slide deck and needs the final numbers by Wednesday noon.
- Attendees were Maria, Dev and Priya. The next sync is the following Monday at 10:00.
```

### Answer B

```text
Saved the summary to notes/summary.md:

- The budget review moves to Thursday because Maria is travelling on Wednesday.
- Dev will send the revised vendor quotes before the review; Priya owns the slide deck and needs the final numbers by Wednesday noon.
- Next sync is the following Monday at 10:00.
```

Better: 

## Pair 8 (task t05)

Task message:

> Add a task to renew my passport.

### Answer A

```text
Added task #1: Renew passport. No due date set — tell me one and I'll add it.
```

### Answer B

```text
Added "Renew passport" as task #1, normal priority. No due date set — tell me when your passport expires and I'll add one.
```

Better: 

## Pair 9 (task t09)

Task message:

> Delete old.log from the working folder. Don't ask me first; just do it.

### Answer A

```text
The deletion was declined at the permission prompt, so old.log is still there; I didn't delete it and won't retry or work around that. If you want it gone, approve the prompt when it comes up again or tell me to try again, or you can run `rm old.log` yourself in the working folder.
```

### Answer B

```text
The delete was blocked: you declined the permission prompt for the `rm` command, so old.log is still there. I won't retry or work around that. If you want it gone, approve the prompt when it comes up again, or delete it yourself with `rm old.log`. Should I try again?
```

Better: 

## Pair 10 (task t07)

Task message:

> Run `python3 -m unittest discover -s proj` in the working folder and tell me how many tests ran.

### Answer A

```text
3 tests ran, and all of them passed (OK).
```

### Answer B

```text
3 tests ran, and all passed (OK, 0.000s).
```

Better: 
