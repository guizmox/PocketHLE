# PocketHLE changes to rmp3 0.3.1

Source: the published rmp3 0.3.1 crate and its bundled minimp3 source.
Both upstream license files are included.

The wrapper exposes the Layer III overlap, synthesis history, header and bit
reservoir for checkpoints. The saved state contains no pointers. The wrapper
also initializes decoder and frame-info storage before use. The native decoder clears unused synthesis lanes before saving filter
history so checkpoints contain deterministic, initialized data. Build flags
are unchanged.
