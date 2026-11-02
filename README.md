# ErisDock

A dock for macOS that rises out of the bottom edge of the screen when the pointer reaches it: a
black body with the notch's inner corners where it meets the edge, in place of the native Dock's
glass. It shows the apps that are running, as the native Dock does, and tucks back into the edge
once the pointer leaves. Rust, on AppKit and Core Animation through the `objc2` crates.

- **Rising**: rest the pointer on the bottom edge of a screen, within reach of where the dock
  sits there, and it rises on that screen. It grows out of the edge as a shallow bump, rising
  faster than it widens, so it stretches up narrow, overshoots, and settles out to its full
  width. Its top corners are rounded and its foot flares into the edge through concave fillets,
  all with Figma's corner smoothing. Icons ride up with its top edge and fade in on the way. It
  stays up while the pointer is near and tucks away shortly after it leaves. An edge another
  screen carries on below doesn't raise it.
- **Icons**: every app with a regular activation policy, Finder first and the rest in the order
  they launched. A new app pops in at the end and a quitting one shrinks out of the row, its
  neighbours closing over. Each running app has a dot beneath it, which stretches into a bar
  under the frontmost app; hidden apps are dimmed. Pointing at an icon lifts it out of the dock
  and shows the app's name above it.
- **Clicking** an icon squashes it and brings its app forward as the native Dock does: unhidden,
  active, and asked to reopen, so an app with no windows open makes one.
- **Menus**, on right-click or Control-click, after the native Dock's: the app's windows (a ◆
  for minimized ones; choosing one brings it forward), **Options › Show in Finder**, **Show All
  Windows** (App Exposé), **Hide** and **Quit**, which Option turns into **Hide Others** and
  **Force Quit**. Finder's has **New Finder Window** and no Quit. Right-clicking the dock between
  icons offers **Open at Login** and **Quit ErisDock**.

The window list needs ErisDock allowed under System Settings › Privacy & Security ›
Accessibility; until it is, menus offer **Allow Window List…** in its place. Show All Windows asks
the native Dock for App Exposé through `CoreDockSendNotification`, a private call ErisDock looks
up when it starts and leaves out if it's gone.

## Building and running

```sh
scripts/install.sh            # builds build/ErisDock.app, copies it to ~/Applications, starts it
scripts/native-dock.sh hide   # keeps the native Dock out of the way
```

`scripts/build.sh` alone builds the app. It's signed ad hoc, and macOS ties Accessibility access
to the signature, so each rebuild has to be allowed again; `ERISDOCK_SIGN=<identity>` signs it
with a lasting identity instead.

`cargo test` checks the springs, the outline and the layout. `cargo run -- --snapshot dock.png`
draws the dock rising, hovered, pressed and tucking away, frame by frame, into one PNG, with the
apps that are running, without putting anything on screen.

## The native Dock

ErisDock leaves the native Dock running: it draws Mission Control, App Exposé and Launchpad.
`scripts/native-dock.sh hide` turns its autohide on and sets its delay to 60000 seconds (1000
minutes), so it never comes up under the pointer. `restore` puts its usual delay back.

## Settings

Sizes scale from the icon size, which is the native Dock's own (`tilesize`) unless set. Any of
these can be set with `defaults write com.eriskii.ErisDock <key> -float <value>` and are read at
launch:

| Key | Default | |
| --- | --- | --- |
| `iconSize` | the native Dock's | |
| `spacing`, `padding` | 0.18, 0.26 × icon | between icons; between the outer icons and the sides |
| `topPadding`, `bottomPadding`, `dotSize` | 0.2 × icon; that + dot + 2; 0.08 × icon | |
| `cornerRadius`, `flareRadius` | 0.46, 0.26 × icon | the top corners; the fillets at the foot |
| `smoothing` | 0.6 | corner smoothing, 0 for circular corners |
| `collapsedWidth` | 0.45 | the width it rises from, as a fraction of its full width |
| `riseResponse`, `riseDamping` | 0.42, 0.58 | the rise's spring: period in seconds, damping fraction |
| `spreadResponse`, `spreadDamping` | 0.5, 0.62 | the widening's spring |
| `revealDelay`, `hideDelay` | 0.06, 0.35 | seconds on the edge before it rises; away before it tucks in |
| `triggerMargin` | 48 | how far beside the dock the edge still raises it |

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
