# Ledgerline

Ledgerline is an expense API for teams that don't want another finance dashboard.
Each receipt is parsed when it is uploaded, and the totals are stored in Postgres.
The rules engine decides which category a charge belongs to.
The webhook retry backoff schedule lives in ledger.toml.
Here's where it gets interesting: approvals happen in Slack.
This matters because finance teams hate surprises at the end of the month.
The sync layer is the engine of the whole product.
Run `ledger import --bank chase` to pull the last 90 days of transactions.
Ledgerline never double-counts a refund, and nothing leaves your VPC.
You set the monthly limit with `ledger limit 500`.
In practice, the overall experience of managing spend becomes significantly smoother and more predictable for everyone involved.
Duplicate charges are flagged by the matcher, not by the bank.
The policy check surfaces the shape of each team's spending.
You approve each charge over $500 in the #spend channel.
Categories are case-insensitive.
Rejected receipts stay in the inbox until you fix them.
Missing fields come from the bank feed.
Only admins can delete a ledger.
Do not paste your API key into a support ticket.
Refunds do not count toward the monthly limit.
If a receipt has no date, Ledgerline uses the upload date.
You cannot undo a deleted ledger, so export it first.
Ledgerline does not read PDF receipts yet, so upload a PNG or JPEG.
A 404 means the expense ID does not exist in this ledger.
Charges above $10,000 need a second approver, not just the admin.
The old docs said the limit resets on your billing date, but it resets on the 1st.
Never run `ledger purge` against the production database.
No receipt is needed for charges under $25.
The import skips transactions that have no merchant name.
Unless you set a limit, Ledgerline approves every charge.

## Status

The crate does not build right now.
The README lists no golden rules yet.
Nothing was committed, and the plugin was left untouched.
I did not use the plannotator library.
This is a claim to test, not a proven result.
It's not a bug — it's a design choice.
No setup. No config. Just run it.

## Comments

The portions are generous for the price point, and you can really customize your bowl without any judgment or extra fees.
The process of extracting particles from the source is not 100% efficient, but you still get a beam out.
What really stood out to me was the attentive staff who checked in just enough to ensure we were comfortable without being intrusive.
At one time, builders didn't understand this process.
The ambiance is casual and lively without feeling loud or overwhelming.
Whatever OP does, OP must not try to return the money if asked, let the bank take it back.
The lighting is moody and sophisticated without being dark and dingy.
And for the same reason suction cups won't work in a vacuum.
Service was attentive without being overbearing.
Sometimes the bleeding is excessive and/or doesn't stop.
They're not just pouring liquor—they're creating experiences.
But you can't put a tourniquet on your uterus very easily, so uncontrolled bleeding can be really hard to stop.
So, it’s not just the oil price that determines gas prices, but a mixture of many costs layered on top.
You don't see their beers on tap at many other bars, and you don't see their cans for sale everywhere.
The real issue isn't that Austrians are wrong about everything—their insights on incentives and unintended consequences are valuable.
The beers themselves are pretty good, but they are not outstanding.
Honestly, it's usually not intentional lying—more like optimistic wishful thinking.
They are not in an industrial park but rather in the downtown area so you have no problem getting food.
The menu offers a great variety of options, and I decided to try the house special, which did not disappoint.
When you give birth, it's not just to a baby - it's also to the entire lining of your uterus.
Second, accelerators are incredibly selective about what they actually look at.
A lot of you were really expecting an evolutionary argument, but I really don't think there is one here.
The detector equipment then looks for very particular signatures that match what they're hunting for.
Interesting perspective, reminds us even the creators face challenges – humility keeps us innovating and evolving.
