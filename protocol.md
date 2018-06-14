# Palace Server Communication
Communication with the palace server is done over a websocket. There are NO HTTP endpoints in this server.

## Communication Requirements

All communication over the websocket is done with binary messages. Sending a text message will result in the connection being closed immediately.

All communication over the websocket should be done in encoded JSON messages; the specific messages are documented below. Sending an unknown message will result in the connection being closed immediately.

Each client is assumed to map to one and only one player at any given time. This will likely be enforced by the server soon, but for now controlling two players on one websocket will result in undefined behavior and should be avoided.

There are currently no rate limits but that is extremely likely to change in the future.

## Incoming Messages (from Client)

These are messages sent from the client to the server.

#### NewLobby
```json
{
  "NewLobby": {
    "max_players": 4,
    "password": "",
    "lobby_name": "Matt's Lobby",
    "player_name": "Matt"
  }
}
```

##### Response
[NewLobbyResponse](#NewLobbyResponse)

##### Requirements
Max players must fall in the range [2, 255].
Lobby name must not be blank.
Player name must not be blank.

##### Description
Upon creating the lobby: a player is created, inserted into the lobby, and the player id is returned. This means that a client should not try to join a lobby after creating it, as they have already joined.

#### JoinLobby

```json
{
  "JoinLobby": {
    "lobby_id": "e789392abcaaac8aad74b5fd4d4f971",
    "player_name": "Matt",
    "password": ""
  }
}
```

##### Response
[JoinLobbyResponse](#JoinLobbyResponse)

##### Requirements
The player name must not be blank.
The lobby id must refer to an extant lobby. Furthermore, the lobby must not be full and the lobby must still be looking for players (the game has not started.)
The password must match the password set by the lobby owner.

##### Description
This is sent to join a lobby. The lobby id can be found by querying the list of lobbies ([ListLobbies](#ListLobbies)), or can be obtained from a client that has created a lobby. After joining a lobby, the client should be prepared to start receiving [game events](#Events), starting with [GameStartedEvent](#Events).

#### ListLobbies
```json
"ListLobbies"
```

##### Response
[ListLobbiesResponse](#ListLobbiesResponse)

##### Requirements
None.

##### Description
This is sent to retrieve a list of lobbies. This is how one finds lobby ids which can be used to join a lobby ([JoinLobby](#JoinLobby).)

#### StartGame
```json
{
  "StartGame": {
    "lobby_id": "958bf2645045de4eacd96d8deab7db5a",
    "player_id": "54c26676175c633b4c904b7369729e10"
  }
}
```

##### Response
[StartGameResponse](#StartGameResponse)

##### Requirements
The lobby id must refer to an extant lobby and the game for that lobby must not be started already.
The given player id must be the owner of the lobby.

##### Description
This is sent to start a game. Immediately after starting a game, the client should be prepared to receive [game events](#Events), starting with [GameStartedEvent](#Events).

## Outgoing Messages (from Server)

### Global

Global messages the client should be prepared to accept at any time.

#### InternalServerError

```json
"InternalServerError"
```

##### Description
This can be sent in response to any message, or in place of any event, and represents an error that can not be handled by the client. On encountering this message, the client should disconnect and attempt to open up a new connection. If this succeeds, the client may attempt to reconnect to the lobby ([Reconnect](#Reconnect).) If either the new connection or lobby reconnection fails the client should give up contacting the server.

If you are encountering this error, consider diagnosing the issue and reporting a bug. It could also simply indicate that the server is overloaded.

### Events

Events that the client should expect while playing a game.

### Responses

Responses that the client can expect after sending the corresponding message.
