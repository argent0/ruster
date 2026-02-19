 ## Requirement

 Make skills be managed per-session.

 Add a skill to manage skills in a session:
    `add`: adds a skill description to the context
    `list`: lists skills loaded,
    `search`: searches available skills 
    `remove`: Remove a skill from all the messages in the sessions' history. Also from the sessions' skill
    `ban`: prevent from being loaded in the future
    `unban`: ...

So for a session there will be a list of skills that were injected in the context at some point.
The information on injected skills should be tracked along the sessions' message history.

Plan this carefully, test it, commit and push.

## Config

Let the user configure the skills to be initially loadad in a new sesssion.
