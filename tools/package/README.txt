Krebs
=====

Krebs is a private server for the game Screeps, with lobbies and a Battle Royale mode.
It runs on your own PC.

In Screeps you do not steer units by hand. You write a program in JavaScript, your bot,
and it plays for you. In a match on Krebs everyone brings a bot, and the bots fight it out
on one map. A simple starter bot comes in this zip, so you can try a match without
writing any code.

Krebs is an early proof of concept. Expect rough edges. Matches have no final standings
and no replays yet.

Krebs is not made by or connected with the makers of Screeps.


What you need
-------------

- Windows 10 or 11, 64-bit.
- The game Screeps: World on Steam. Krebs is only the server, and the game is how you play.


Start
-----

1. Unzip this whole folder first. Right-click the zip and choose Extract All.
   Do not start Krebs from inside the zip, and do not put it in Program Files.
   Documents or the Desktop are good places.

2. Double-click Krebs.exe.

   Windows may show a blue box that says "Windows protected your PC". It does this
   because Krebs is new and has no paid signature, not because anything was found.
   Click "More info", then click "Run anyway". You should only need to do this once.

3. A black window opens and says "Krebs is running". Keep it open while you play.

4. Open Screeps in Steam. On the server list, switch from Community servers to Private server.
   The list always opens on Community servers, so switch before you click anything.

5. Enter Host 127.0.0.1 and Port 21025, then press Connect.

To stop, close the black window. Your world is saved in the data folder beside Krebs.exe.
Any match that is running ends when you close it.


Playing a match
---------------

Press New lobby, or join a lobby that is open. Pick the folder with your bot's Screeps
code, or drop a folder or a zip on the window, up to 10 MB.
No bot yet? Pick the starter-bot folder that came in this zip.
Press Ready. When everyone is ready the match counts down, everybody picks a room and
places a spawn, and the bots play. Hold Leave server to leave.

One Krebs runs up to 4 matches at a time. A lobby past that is told to wait for one to end.


Playing with friends
--------------------

1. Double-click "Let friends join.bat" instead of Krebs.exe.

2. Windows Firewall may ask about Krebs. Allow it, or nobody else can connect.

3. The black window lists this PC's addresses. Give a friend the first one and Port 21025.
   They do step 4 and step 5 above with that address as the Host.
   If the first address does not work, try the next one.

Your friends need a bot too. If they have none, send them the starter-bot folder.

If friends cannot connect, look at Windows Firewall first, even if it never asked you
anything:

1. Open the Start menu and search for "Windows Defender Firewall with Advanced Security".

2. Click Inbound Rules. Look for rules named krebs or Krebs.exe with a red sign in front.
   Right-click each one and choose Delete.

3. Close Krebs and double-click "Let friends join.bat" again. Allow Krebs if Windows asks.

If that does not help and this is your home network, open Settings, then Network & internet,
and set your network to Private.

These addresses work for people on your own network, at home for example. For friends
over the internet, you have to forward port 21025 to this PC on your router, and give them
your public address. Your router's manual says how. Krebs cannot do this for you.

Be careful who you invite. Krebs cannot check who people are, and anyone who reaches the
port can sign in as anyone. Nothing is encrypted. Only share it with people you trust.
For each player, your PC asks Steam for their public profile picture.
Every match runs on your PC, so more players means more work for it.


If something goes wrong
-----------------------

Krebs says what went wrong in its window, and what to do about it.

"Port 21025 is already used by another program": close that program, or give Krebs another
port. Open krebs.json in Notepad and change the number, so that the whole file reads:

    {
      "http": { "port": 21030 },
      "mods": ["bare"]
    }

Use the new port in step 5, and give it to your friends too.

"Krebs could not read its settings": krebs.json has a mistake. Fix it, or delete the file.
Krebs then uses its normal settings, port 21025 and all.

"Krebs could not save your world": the disk may be full, or the data folder cannot be written
to. Keep the window open and fix that. Krebs tries again by itself and says when it works.
If you close the window first, you lose what happened since the last save.

"An error occured while logging into your Steam account": this comes from the Screeps
client and usually means Steam stopped answering. Restart Steam and try again.

More help and the source code:
https://gitea.defaul.to/DefaultO/krebs


Licence
-------

Krebs is under the MIT licence, see LICENSE.txt. Parts of it come from other projects and
stay under their own licences, see THIRD_PARTY_NOTICES.txt.
