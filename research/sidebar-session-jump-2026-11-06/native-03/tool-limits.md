# Native input tool limits

The reviewed source for this attempt is `5916f78cf3ff1755695d45ff6dc648933641848f`, packaged by `just gui` from the integration checkout.

The native tool rejected `pressKey('super+1+0')` with `keyPressIncludedMultipleNonModifierKeys(ComputerUse.XKeysymString.KeyPress(keys: [ComputerUse.XKeysym.super, ComputerUse.XKeysym.one, ComputerUse.XKeysym.zero]))`. No navigation followed that rejected call. Separate `pressKey` calls release the modifier between calls, so they cannot prove a two-digit choice within one hold.

Earlier serialized hold observations and the unavailable system capture interface remain tool limitations, not successful visual acceptance. A bounded physical-key handoff was requested with the ten-Agent fixture open: hold Cmd, type `1`, retain the hold for capture, then type `0` and release. Its actual outcome must be recorded separately.

Kyle tried the interaction and requested single digits only. The two-digit capture request is superseded; no successful prefix or two-digit native outcome is claimed. The owned GUI was closed and cleaned before implementing the revision.
