Krebs on macOS and Linux
========================

Read this first. README.txt beside it was written for Windows, and this page says what is
different here. Everything else in README.txt holds on macOS and Linux too.

This archive was made with tools/package.sh. The downloads the Krebs author publishes are
for Windows, and a build for macOS or Linux has not been through the same tests.


Start
-----

1. Unpack the archive and keep the Krebs folder together.

2. Open a terminal in the Krebs folder and run:

       ./krebs

   Where README.txt says to double-click Krebs.exe, this is what it means. The "black
   window" is this terminal: keep it open while you play.

3. Carry on with step 4 of Start in README.txt.

To stop, press Ctrl+C in the terminal. Your world is saved in the data folder beside krebs.

macOS stops a downloaded program that Apple has not checked, and Krebs has no paid
signature. If macOS refuses to open krebs, allow it under System Settings, Privacy &
Security, Open Anyway. Or take the download mark off the whole folder, in the terminal:

       xattr -dr com.apple.quarantine .


Playing with friends
--------------------

Where README.txt says "Let friends join.bat", run this in the terminal:

       ./krebs --friends

The file "Let friends join" in this folder does the same when you open it.

Krebs asks Steam who signs in through the program curl. macOS has it. On Linux, install it
if the command curl is not found.

The firewall steps in README.txt are for Windows. If friends cannot connect, look at the
firewall of your own system, and allow incoming connections to port 21025.


Where the two differ in words
-----------------------------

Krebs prints its messages as it does on Windows. Krebs.exe means the krebs program in this
folder, "this PC" means this computer, and Notepad means any text editor. To paste the
Steam key, use your terminal's own paste. To find this computer's address when Krebs
cannot, run ifconfig, or ip addr on Linux.
