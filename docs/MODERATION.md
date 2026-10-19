# Moderating friends and chats

Players report messages from the chat (D-064). Reports and the tools to act on them
are on the server only: this repository's workflow logs are public, so nothing a player
wrote is ever shown there. Run the commands over SSH as the service user:

```sh
sudo -u lapi-friends /home/www/lapi/friends/current/lapi-friends <command>
```

| Command                           | What it does                                                  |
| --------------------------------- | ------------------------------------------------------------- |
| `reports [--all] [--limit N]`     | Open reports (all with `--all`), newest first, as JSON lines  |
| `resolve <report> [note]`         | Marks a report handled, with a note for later                 |
| `remove-message <message>`        | Removes a message for everyone; its reports keep their copy   |
| `mute <account> <hours> [reason]` | The account reads but sends no messages, requests or groups   |
| `ban <account> [days] [reason]`   | The account gets nothing from friends; without days, for good |
| `lift <account>`                  | Ends a mute or ban                                            |
| `sanctions`                       | Mutes and bans in force                                       |

Each report shows the reporter, the sender, the room and a copy of the message as it was
when it was reported, and whether the message is still there. Account IDs are what the
reports show; `sudo -u lapi-oauth /home/www/lapi/oauth/current/lapi-oauth find <id>`
tells whose they are. The running service tells the people concerned within 15 seconds:
a removed message leaves the open chats, a banned account's connections close, and the
launcher shows a muted or banned player until when.

To switch an account off everywhere (sign-in included), use
`lapi-oauth disable <id>`; `lapi-oauth enable <id>` undoes it.

Players can delete their account themselves (Accounts → _Delete account…_). Their
friends, messages and the reports they filed go with it; reports about their messages
keep their copy.
