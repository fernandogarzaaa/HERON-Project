# Owner trial record (G9)

G9 is met when, over 14 consecutive days, at least 12 days have a tool call you caused: an
executed `audit` row in a session whose origin is `user` or `telegram`, and the trial has zero
severity-1 incidents (an unapproved state change, data loss, or an exposed secret).

Fill in the first block before the trial starts. Count the days at any point with:

    bash eval/owner-use.sh ~/.heron <start> <end> --min 12

`<end>` must be exactly 13 days after `<start>`. The count needs data format 3, which U2 adds.

## Trial

- Tool Heron replaces (Q4):
- Start date (YYYY-MM-DD):
- End date (YYYY-MM-DD, 13 days after the start):

## Days

One line per day. Note what you used Heron for, and anything that went wrong.

| Day | Used Heron? | What for | Incidents |
|---|---|---|---|
| 1 | | | |
| 2 | | | |
| 3 | | | |
| 4 | | | |
| 5 | | | |
| 6 | | | |
| 7 | | | |
| 8 | | | |
| 9 | | | |
| 10 | | | |
| 11 | | | |
| 12 | | | |
| 13 | | | |
| 14 | | | |

## Result

- `qualifying_days`:
- Severity-1 incidents (count and a line each):
- G9 met (12 or more days and no incident)?
