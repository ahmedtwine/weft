# Tidepool

Tidepool is a small backup tool for people who don't want to think about backups. It quietly watches the folders you care about and copies each change to a bucket you own, with no daemon to babysit and no dashboard to check.

Here's where it gets interesting: Tidepool never stores a full copy of your files. Each change is split into chunks, and the chunks are deduplicated before they are uploaded. The shape of your data lives in a small index, and that index is the engine of the whole system.

This matters because restores are where most backup tools fall apart. With Tidepool, a restore isn't a special mode, it's just a sync in the other direction. You don't need to remember flags, and nothing is overwritten without your say.

The point is simple: your backups should hold up when you need them, and they should stay yours.
