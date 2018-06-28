# Palace Server Communication
Communication with the palace server is done over a websocket. There are NO HTTP endpoints in this server.

## Communication Requirements

All communication over the websocket is done with binary messages. Sending a text message will result in the connection being closed immediately.

All communication over the websocket should be done in UTF-8 encoded JSON messages; the specific messages are documented below. Sending an unknown message will result in the connection being closed immediately.

Each client is assumed to map to one and only one player at any given time. Each connection will only receive game updates from the last game it (re)connected to.

There are currently no rate limits but that is extremely likely to change in the future.

Once the game has started, each turn must be taken within 45 seconds (the server grants some additional leeway to account for factors such as latency, but this should not be relied upon.) Failure to do so will result in the player being ejected from the game, with an Ai taking the now empty slot.

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
* Max players must fall in the range [2, 255].
* Lobby name must not be blank.
* Player name must not be blank.

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
* The player name must not be blank.
* The lobby id must refer to a valid lobby.
* The lobby must not be full.
* The lobby must still be looking for players (the game has not started.)
* The password must match the password set by the lobby owner.

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
* The lobby id must refer to a valid lobby.
* The game for that lobby must not be started already.
* The player id must refer to the owner of the lobby.

##### Description
This is sent to start a game. Immediately after starting a game, the client should be prepared to receive [game events](#Events), starting with a [GameStartedEvent](#GameStartedEvent) and a [PublicGameStateEvent](#PublicGameStateEvent).

#### ChooseFaceup
```json
{
  "ChooseFaceup": {
    "lobby_id": "958bf2645045de4eacd96d8deab7db5a",
    "player_id": "54c26676175c633b4c904b7369729e10",
    "card_one": {
      "value": "Two",
      "suit": "Clubs"
    },
    "card_two": {
      "value": "Three",
      "suit": "Diamonds"
    },
    "card_three": {
      "value": "Four",
      "suit": "Spades"
    }
  }
}
```

##### Response
[ChooseFaceupResponse](#ChooseFaceupResponse)

##### Requirements
* The lobby id must refer to a valid lobby.
* The game for that lobby must have started.
* The game for that lobby must be in the setup phase.
* The player id must refer to the player whose turn it is.
* The cards must be valid cards that the player has in their hand or in their faceup cards (mixing from each source is fine.)

##### Description
This is sent once by each player at the beginning of the game (in the Setup phase) to choose which three cards they would like to set as their faceup cards for the duration of the game. After the faceup choosing is accepted by the server, a [PublicGameStateEvent](#PulbicGameStateEvent) will be sent to all players and a [HandEvent](#HandEvent) will be sent to the player who made choose the faceup cards.

#### MakePlay
```json
{
  "MakePlay": {
    "lobby_id": "958bf2645045de4eacd96d8deab7db5a",
    "player_id": "54c26676175c633b4c904b7369729e10",
    "cards": [
      {
        "value": "Two",
        "suit": "Clubs"
      },
      {
        "value": "Two",
        "suit": "Diamonds"
      }
    ]
  }
}
```

##### Response
[MakePlayResponse](#MakePlayResponse)

##### Requirements
* The lobby id must refer to a valid lobby.
* The game for that lobby must have started.
* The game for that lobby must be in the play phase.
* The player id must refer to the player whose turn it is.
* The cards must all share the same value.
* The cards must be in the player's hand if the player has any cards in their hand, or from their faceup cards if they have no cards in their hand.
  * If the player has no cards in either their hand or their faceup cards, the list of cards should be blank. The next facedown card will be played.

##### Description
This is by each player on their turn when the game is in the Play phase. This message will be sent many times per client per game. After the play is accepted by the server, a [PublicGameStateEvent](#PulbicGameStateEvent) will be sent to all players and a [HandEvent](#HandEvent) will be sent to the player who made the play.

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
