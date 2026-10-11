PocketHLE CI idle-gate candidate fix

Moves the suspended-process idle 1ms sleep outside the shared API gate.
No diagnostic timeout change or skipped tests. Common Windows/Android kernel.
290 WinCE unit tests passed; complete ARM runner test is not locally validated
because Unicorn build requires missing libclang. Confirm in CI.
Apply over the cumulative default-texture patch. Run cargo +1.90.0 fmt --all
before committing; no workflow modification is required.
