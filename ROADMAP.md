# Roadmap

What ErisDock still needs to do what the native Dock does, roughly in the order it's worth
doing. The native Dock keeps running underneath for Mission Control, App Exposé and the Apps
launcher, so those aren't on the list.

## Done

- Running apps, hidden ones dimmed.
- Clicking an app brings it forward as the Dock does, reopening it when it has no windows.
- Menus: windows, Options, Show All Windows, Hide / Hide Others, Quit / Force Quit.
- Pinned apps ("Keep in Dock"), imported once from the native Dock, with a switch to turn pinning
  off and a guard, on by default, against apps pinning themselves.
- Drag and drop: drag apps to reorder them, drag a pinned app up off the dock to unpin it, drop
  an app from Finder between icons to pin it, drop files on an app to open them with it.
- Notification badges.
- Marks under icons for what each app's windows are doing (focused, open here, minimized here,
  on other desktops, no windows, hidden), each picked from dots, bars and nothing.
- Bouncing while a pinned app opens.
- Settings: a config file and a window, and three sets of animations.

## Everyday gaps

- **Bouncing for attention**, when an app asks for it (`requestUserAttention`). Those requests
  go to the native Dock, so catching them needs digging.
- **Bouncing on any launch**, not just from the dock: apps opened from Spotlight or Finder
  could bounce too, while they aren't finished launching.

## Native behaviours

- **Click shortcuts**: Option-click to hide the others, Command-click to show the app in Finder,
  click and hold to open the menu.
- **Apps' own menu items**, like Safari's or Terminal's New Window. Apps give these to the
  native Dock through a private channel that would need reverse-engineering.
- **More of the window list**: windows on other Spaces, Options › Assign To Desktop and Open at
  Login per app.
- **Keyboard and accessibility**: a key to bring the dock up, arrow keys through it, VoiceOver
  labels.
- **Dragging feedback**: the poof cloud when an app is dragged off, spring-loaded opening of an
  app when a file is held over it.

## Harder

- **Minimized windows** go into the hidden native Dock. Tiles for them can come from the
  Accessibility API, thumbnails need Screen Recording, and sending the minimize animation here
  instead would mean patching the Dock process, as yabai does.
- **Live tiles**: progress bars, Calendar's date icon, anything an app draws into its Dock tile.
  Only the native Dock gets these; some may not be reachable.
- **Folders and stacks** (like Downloads) as fans and grids, and the recent apps section.
- **Layout options**: magnification, the left or right edge, and following the pointer to
  another display.
