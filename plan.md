# Nostalgia

Nostalgia is a client and server project for manage and downloading retro games to a client and saving cloudsaves in the server. This project should run on windows and steamos/linux and should be controlled by a mouse, keyboard, and controller

## Server
The server is used for providing games, game downloads, managing cloudsaves, managing users, and managing games.
The server should have a webserver where an admin can easy manage new games and users.

Setting should live in a settings file.

### Settings
- Admin Username
- Admin Password
- Game Library Directory
- IGDB Client Id
- IGDB Client Secret

### Game Library
The game library consists of two parts the games and emulators

The games live a following structure:
```
./games/{platform}/{Name of Game} [igdbid-{igdb-id}]/Game.iso
```

The platform folders are based on the romm folders and should be created when not existing. NEVER delete files

Games can feature multiple files like multidisk games or cue/bin combinations. Serve all files from a game folder to the client

### IGDB
Every game folder has a igdb id. When scanning for new games download in the background the cover and artworks for the game. These downloaded assets should be stored in the game folder under "igdb" since they are need to be downloaded to the client. Also information and description of the games shoud be stored in the game folder.

### Hasheous
When scanning for games also check the games hash with the hasheous database. This is only and info for the admin. Every scan can skip this part. Is hash exists or not is not relevant for the client.

### Users
Everyone can register a user over the client and change passwords.
Every users has its own game library that they added from whole game library. like in Steam

### Cloudsaves
For every user there should be cloudsaves saved. This cloudsaves should be versioned. NEVER delete Cloudsaves
Uploading happens from the client. The server should automaticly send the newest cloudsave from a user when a client is open.

Cloudsaves are not stored per game but per emulator. Since not every emulator has that feature

### Emulators
The server should provide emulators. The emulator are stored following:
```
./emulators/{platform}/{Name of Emulator}/*
```

A platform can have multiple emulators.
In every emulator is a "emu.yaml" file
that descrips for the client where the saves are gonna be stored on the device.

## Client

The client allows the user to add games to there library, start the game via emulator in fullscreen, automaticlly or manual download cloudsaves and more.

It must run on windows and steamos/linux

### UI

The UI must be controlled with mouse, keyboard, or controller. In can be opened in a window or fullscreen. It should look like steam or any other modern game launcher. 

There should be 3 tabs.
- Vault
- Library
- Downloads
- Settings

The Vault shows every game on the server. It should also allow the user the filter for games using the data from server that it got from igdb. 
Every game should have a "store page" with infos aboud the games again provided by the server.

The game library shows every game the user added to its library. the user can download and remove games. set game specific settings that should not be stored on the server. Run the game in an emulator.


the settings allows the user to change there password. Add and remove game download locations on the device.

Users should have the ability to logout

download should be able to paused

Emulator should not be shown by default. There should be a checkboxs to show emulators.

also there should be the ability to shown only installed games.

### Cloudsaves

When the client starts it should automaticly download any newest cloudsave the the installed games.

The user should be able to download older cloudsaves

### Emulators
Emulators should be automaticlly added to the users library when a game from this platform is added to its library.

Emulators can be downloaded on its own.

When the emulator is closed and the saves in the emulator saves folder (that are descipted in the emu.yaml) are newer that upload them automaticlty to the server

It the upload failed just try again later.

### Offline gaming
Since the games download also downloads also downloads the artwork and covers. When in offline mode gray out all not download games and show a offline info in the game vault tab. Download games should still show the covers and artwork and should be eabled to be played.
Cloudsaves will then be uploaded then back online

### Playing the game
The client should track any open game and count game time that should then uploaded to the server.
How to start a game in an emulator is set set per platform in the emu.yaml

In the setting users can change emulators and the discs if its a multidisc game for the game

