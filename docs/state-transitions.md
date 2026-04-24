# State Transitions

This document describes the state machines for **tasks** and **intake issues**.

## Task States

```
pending → creating_agent → cloning → running_claude → awaiting_followup → pushing → creating_preview → completed
                                          ↕
                                     (follow-up loop)

Any state → failed
```

| State | Description |
|---|---|
| `pending` | Task created, waiting to be picked up |
| `creating_agent` | Spinning up an agent container |
| `cloning` | Cloning the repo and creating a working branch |
| `running_claude` | Claude is processing the prompt |
| `awaiting_followup` | Claude finished; waiting for user feedback |
| `pushing` | Pushing changes to GitHub |
| `creating_preview` | Building a preview environment |
| `completed` | Terminal. Task finished successfully |
| `failed` | Terminal. An error occurred at any stage |

### Follow-up loop

After Claude produces its initial output the task enters `awaiting_followup`. When the user sends a follow-up message it cycles back to `running_claude`, then returns to `awaiting_followup` once Claude responds. This loop repeats until the user marks the task as complete, at which point it proceeds through `pushing` → `creating_preview` → `completed`.

## Intake Issue States

```
             ┌─────────────────────────────┐
             │                             ▼
(polled) → backlog ⇄ pending → task_created ⇄ review → done
                                    │           │
                                    ▼           ▼
                        failed ─────────────────┘
                       ↙      ↘
                  backlog    pending  (retry)
```

| State | Description |
|---|---|
| `backlog` | Polled from external issue tracker, sitting in queue |
| `pending` | Admin promoted it; eligible for the daemon to pick up |
| `task_created` | Daemon spawned a task; waiting for it to finish |
| `review` | Linked task reached `awaiting_followup` or `completed` |
| `done` | Terminal. Issue is resolved |
| `failed` | Something went wrong; retryable |

### Allowed UI transitions

These are enforced both server-side and in the Kanban board frontend:

| From | Allowed targets |
|---|---|
| `backlog` | `pending`, `done` |
| `pending` | `backlog` |
| `task_created` | `failed` |
| `review` | `done`, `failed` |
| `failed` | `backlog`, `pending` |
| `done` | _(none — terminal)_ |

### Daemon-driven transitions

The intake daemon syncs issue status with the linked task every 30 seconds:

| Condition | Transition |
|---|---|
| Linked task reaches `awaiting_followup` or `completed` | `task_created` → `review` |
| Linked task goes back to `running_claude` (user sent follow-up) | `review` → `task_created` |
| Linked task marked `completed` | `review` → `done` |
| Linked task fails | `task_created` → `failed` |

### Retry behavior

Moving a failed issue back to `backlog` or `pending` clears the `task_id` and `error_message`, allowing the daemon to spawn a fresh task.

### Concurrency control

Two limits gate how many tasks can run at once:

- **Global**: `INTAKE_MAX_GLOBAL_CONCURRENT` — total issues in `task_created` + `review` across all sources.
- **Per-source**: `max_concurrent_tasks` on each intake source — scoped to that source only.

The daemon checks both before promoting a `pending` issue to `task_created`.
