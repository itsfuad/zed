---
title: Agent Profiles - Zed
description: Configure Zed Agent profiles for model selection, built-in tool availability, and MCP tool availability.
---

# Agent Profiles

Agent profiles control how the [Zed Agent](./zed-agent.md) behaves in a thread. A profile can set a default model, restrict subagent models, and choose which [built-in tools](./tools.md) and MCP tools are available.

Use [Tool Permissions](./tool-permissions.md) to control allow, deny, and confirm behavior for permission-gated tools. Subagent model restrictions are configured separately in the profile.

## Built-in Profiles {#built-in-profiles}

Zed includes three built-in profiles:

- `Write`: enables tools for reading, editing, and running commands.
- `Ask`: focuses on read-only codebase questions.
- `Minimal`: uses no project tools.

## Configure Profiles {#configure-profiles}

Open the profile selector in the [Agent Panel](./agent-panel.md), then click `Configure`.

You can also run {#action agent::ManageProfiles} from the command palette.

From the profile modal, you can:

- create a custom profile
- fork an existing profile
- configure a profile default model
- configure built-in tools
- configure MCP tools
- delete custom profiles

## Profiles and Settings {#settings}

Profiles are stored under `agent.profiles` in your settings.

```json [settings]
{
  "agent": {
    "profiles": {
      "ask": {
        "name": "Ask",
        "tools": {
          "read_file": true,
          "grep": true,
          "terminal": false,
          "edit_file": false
        },
        "enable_all_context_servers": false,
        "context_servers": {},
        "default_model": {
          "provider": "zed.dev",
          "model": "claude-sonnet-4-5"
        }
      }
    }
  }
}
```

The exact model IDs and provider IDs depend on your configured [LLM Providers](./llm-providers.md).

## Subagent Model Restrictions {#subagent-model-restrictions}

Set `allowed_subagent_models` on the active profile to control which models native
subagents may use. Each entry is an exact `provider/model-id` from the native Zed
agent entry returned by `list_agents_and_models`.

Open your settings file with {#action zed::OpenSettingsFile}. For example, this
configuration prefers Haiku for subagents and permits only that model:

```json [settings]
{
  "agent": {
    "subagent_model": {
      "provider": "anthropic",
      "model": "claude-haiku-4-5"
    },
    "profiles": {
      "write": {
        "name": "Write",
        "allowed_subagent_models": ["anthropic/claude-haiku-4-5"],
        "allow_subagent_model_override": false
      }
    }
  }
}
```

Replace the model ID with one available from your configured provider.

- Omitting the allowlist permits all available models.
- An empty list permits no models unless you enable and approve an exception.
- A nonempty list permits only the listed provider/model combinations.
- Malformed allowlists block subagent model use until you correct them.

The allowlist applies to explicit `spawn_agent.model` selections, the configured
`agent.subagent_model`, and inherited parent models. Zed applies the preferred
subagent model when the tool omits `model`. A preferred model that is unavailable
causes the call to fail instead of silently using the parent model.

To prevent subagents from using the parent model, omit it from the allowlist and
keep `allow_subagent_model_override` set to `false`. To require one specific
model, configure it as `agent.subagent_model` and make it the only allowlist entry.

### User-Approved Overrides {#subagent-model-overrides}

Set `allow_subagent_model_override` to `true` to let a subagent request a model
outside the allowlist. Zed asks for confirmation showing the exact provider/model
ID. The default is `false`, which rejects outside-list models.

Approval authorizes that model for the specific live subagent session, including
follow-up messages. It does not authorize other sessions or descendants, and
does not alter your settings. Changing the profile or reopening the session
clears the approval. Setting the override flag to `false` blocks an outside-list
model even if you previously approved it.

Resumed sessions keep their model and are checked against the caller's current
profile. Model restrictions are also checked before subsequent subagent requests,
including refusal fallback, compaction, thread summaries, and title generation.
Include configured compaction and summary models in the allowlist if you want to
permit those requests. Requests already in progress can finish.

## Profiles vs. Tool Permissions {#profiles-vs-tool-permissions}

| Setting          | Controls                                                              | Example                                   |
| ---------------- | --------------------------------------------------------------------- | ----------------------------------------- |
| Agent profile    | Whether a tool is available in a profile                              | Disable `terminal` in a read-only profile |
| Tool permissions | Whether a permission-gated tool call is allowed, denied, or confirmed | Always confirm `terminal` commands        |

If a tool is not available in the active profile, the Zed Agent cannot use it. If the tool is available and permission-gated, [Tool Permissions](./tool-permissions.md) still controls whether the tool call requires approval.

## Agent Path Boundaries {#agent-path-boundaries}

Agent profiles apply to the Zed Agent. External Agents and [Terminal Threads](./terminal-threads.md) do not use Zed Agent profiles unless their integration explicitly supports similar behavior.
