---
title: Command space shortcuts (personal fork)
description: Hold Command to see direct space shortcuts on macOS.
---

This personal macOS fork requests Kitty keyboard event reporting while attached.
Hold either Command key to show `⌘1`–`⌘9` beside the first nine spaces in the
active endpoint’s sidebar order. Press Command plus that digit to focus the space.
The mapping stays fixed until both Command keys are released. Window focus loss
clears the hints. Deleted or disconnected targets do not select another space.

Command+0 keeps Ghostty’s font reset. Command+K searches spaces beyond the first
nine. Letter shortcuts are unchanged. Shortcuts do not activate while a dialog,
popup, or asynchronous copy operation owns input. The compact sidebar uses its
own displayed order. Scrolling does not renumber spaces.

Ghostty must pass Command+1–9 through instead of handling its native tab shortcuts.
For each digit 1 through 9, unbind both `cmd+1` and `cmd+digit_1` (substitute the
digit) using Ghostty’s `unbind` action. Do not remap to text: native Kitty key
press/release events preserve the hold lifecycle. No server restart is needed.

Ghostty supports standalone modifier events through Kitty report-all. Its macOS
input handler omits modifier changes during active IME preedit; hints cannot be
guaranteed during composition. Direct Command+digit still works when that chord
is delivered. Ordinary text and committed IME text keep their existing routes.
Other terminals must support the same protocol to show hold hints.
