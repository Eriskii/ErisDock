# ErisDock

A dock for macOS that rises out of the bottom edge of the screen when the pointer reaches it: a
black body with the notch's inner corners where it meets the edge, in place of the native Dock's
glass. It shows the apps that are running, as the native Dock does, and tucks back into the edge
once the pointer leaves. Rust, on AppKit and Core Animation through the `objc2` crates.

- **Rising**: rest the pointer on the bottom edge of a screen, within reach of where the dock
  sits there, and it rises on that screen, growing out of the edge as a shallow bump, in one of
  three sets of animations (see *Settings*). Its top corners are rounded and its foot flares into the edge through concave fillets,
  all with Figma's corner smoothing. Icons ride up with its top edge and fade in on the way. It
  stays up while the pointer is near and tucks away shortly after it leaves. An edge another
  screen carries on below doesn't raise it.
- **Icons**: every app with a regular activation policy, Finder first and the rest in the order
  they launched. A new app pops in at the end and a quitting one shrinks out of the row, its
  neighbours closing over. Each running app has a dot beneath it, which stretches into a bar
  under the frontmost app; hidden apps are dimmed. Pointing at an icon lifts it out of the dock
  and shows the app's name above it.
- **Clicking** an icon presses it in and brings its app forward as the native Dock does: unhidden,
  active, and asked to reopen, so an app with no windows open makes one.
- **Menus**, on right-click or Control-click, open just above the icon, after the native Dock's: the app's windows (a ◆
  for minimized ones; choosing one brings it forward), **Options › Show in Finder**, **Show All
  Windows** (App Exposé), **Hide** and **Quit**, which Option turns into **Hide Others** and
  **Force Quit**. Finder's has **New Finder Window** and no Quit. Right-clicking the dock between
  icons offers **Settings…**, **Open at Login** and **Quit ErisDock**.

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

`cargo test` checks the springs, the outline, the layout, the animation sets and the config
file. `cargo run -- --snapshot dock.png [--style squishy]` draws the dock rising, hovered,
pressed and tucking away, frame by frame, into one PNG, with the apps that are running, without
putting anything on screen.

## The native Dock

ErisDock leaves the native Dock running: it draws Mission Control, App Exposé and Launchpad.
`scripts/native-dock.sh hide` turns its autohide on and sets its delay to 60000 seconds (1000
minutes), so it never comes up under the pointer. `restore` puts its usual delay back.

## Settings

**Settings…**, from right-clicking the dock between icons (or opening ErisDock again while
it runs), opens a small window holding the dock up beside it, so each change shows as it's
made: the animation, the icon size, corner smoothing, how long the pointer rests on the edge
before the dock rises and how long it's away before the dock tucks back in, and opening at
login. **Replay Animation** tucks the dock away and raises it again.

Everything is kept in `~/.config/erisdock/config.toml` (or under `$XDG_CONFIG_HOME`), written
the first time ErisDock runs with every setting described and the defaults commented out.
ErisDock picks up changes to it as soon as it's saved; the window writes into it and keeps its
comments. Sizes left out scale with the icon size, which is the native Dock's own unless set.

The animations come as sets:

- **Squishy** (the default): the dock rises before it widens, overshooting tall and narrow
  and then wide, and pressed icons squash flat.
- **Expressive**: Material 3 Expressive's springs. The dock rises and widens
  together, quick and direct, with a little give as it lands; icons pop in with a small
  bounce and shrink slightly when pressed.
- **Standard**: Material 3's standard springs, straight up and out with no bounce.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
