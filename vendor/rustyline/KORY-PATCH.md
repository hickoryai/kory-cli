# Kory terminal integration

This is the crates.io source of Rustyline 18.0.1, under its original MIT license.
The workspace patch keeps the installed CLI reproducible; editing a Cargo cache
would leave other builds with the original bugs.

Local changes:

- `PosixRawReader::select` consumes buffered keyboard input before waiting on the
  descriptor. With an external printer, the original implementation strands all
  but the first key in a burst until further input arrives.
- Unix renderer writes and external-print transactions share the stdout lock with
  the application's footer painter. Prompt redraws reset SGR attributes so agent
  output cannot leave input black or reversed, including with color highlighting off.
- `Helper::resize(prompt_rows)` lets an application reposition terminal chrome and
  request a fresh prompt repaint. The default retains upstream behavior. Kory uses
  it to resize the child dock and the prompt together, on the line editor's thread.

The real PTY and VT100 regression tests live in
`crates/goose-cli/tests/support/dock_terminal.rs`; they exercise rapid typing,
background output, default input colors, child selection, tool navigation, command
history and repeated height/width changes. Remove the patch only when those tests
pass against a replacement dependency with equivalent integration support.
