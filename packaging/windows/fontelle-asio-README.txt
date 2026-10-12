fontelle-asio - the ASIO(R) helper for Fontelle
===============================================

fontelle-asio.exe is a separate program from Fontelle, under a different
licence.

  * Fontelle (fontelle.exe) is licensed MIT OR Apache-2.0 (LICENSE-MIT,
    LICENSE-APACHE).
  * fontelle-asio.exe is licensed under the GNU General Public License,
    version 3 only (licenses/fontelle-asio/LICENSE). It is built with
    Steinberg's ASIO SDK 2.3.4, which Steinberg offers under GPLv3; the
    SDK's own terms are in licenses/fontelle-asio/STEINBERG-ASIO-SDK-LICENSE.txt.

What it does
------------
When you choose "ASIO(R) (via fontelle-asio helper)" under Settings ->
Audio output, Fontelle starts this program. It loads your audio
interface's ASIO driver and passes audio between the driver and Fontelle
through shared memory, one buffer at a time. It never loads Fontelle's
code or any plugin, and Fontelle never loads its code: the two exchange
plain numbers. If it stops, Fontelle plays through Windows audio instead
and says so. You can delete fontelle-asio.exe; Fontelle then simply does
not offer ASIO.

Source code
-----------
The complete source code of this program, and of the protocol it speaks,
is in Fontelle's public repository:

    https://github.com/Fopull-LLC/DAW-Fontelle
    crates/fontelle-asio-host   (this program, GPL-3.0-only)
    crates/fontelle-audio-ipc   (the protocol, MIT OR Apache-2.0)
    docs/asio.md                (the protocol, written down; how to build)

Use the tag named by the version in this folder's name (v<version>). The
same source is attached to that release as
fontelle-asio-source-<version>.tar.gz:

    https://github.com/Fopull-LLC/DAW-Fontelle/releases

To build it: cargo build --release --locked -p fontelle-asio-host

This program comes with ABSOLUTELY NO WARRANTY; see sections 15 and 16 of
the GNU General Public License. You may redistribute it under the terms of
that licence.

ASIO is a registered trademark of Steinberg Media Technologies GmbH.
The ASIO Compatible Logo, which a text file cannot show, is in
licenses/trademarks/asio/ beside its NOTICE (Steinberg's mark, not covered
by Fontelle's or this program's licence), and Fontelle shows it in
Settings -> Audio output and in its start menu.
