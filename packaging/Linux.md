# Still on Linux

The Linux binary requires glibc 2.39 or newer, GTK 4.12 or newer, libadwaita 1.4 or newer, Cairo, Pango, GdkPixbuf, and GLib. Capture uses grim and slurp. Clipboard output uses wl-copy, and previews use notify-send.

From an extracted tarball, run `./bin/still` for the editor, `./bin/still --capture` for region capture, or `./bin/still --settings` for preferences. The Debian and RPM packages install a launcher and the `still` command.
