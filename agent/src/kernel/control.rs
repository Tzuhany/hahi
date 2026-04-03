// ============================================================================
// Control Resume — Translating User Decisions Back into Conversation Context
//
// When the agent pauses for user input (permission request, plan review), the
// gateway eventually calls ResumeRun with a ControlResponse proto. This module
// translates that proto response back into a system-reminder string that gets
// injected into the conversation so the LLM understands what the user decided.
//
// The `kind` field on PendingControl matches the `type` field emitted in the
// ControlRequest event — both sides must agree on these string values.
// ============================================================================

use crate::common::PendingControl;
use crate::kernel::plan_mode;

pub fn resume_message(
    pending: &PendingControl,
    control: &hahi_proto::agent_event::ControlResponse,
) -> std::result::Result<String, String> {
    use hahi_proto::agent_event::control_response::Response as ControlResponseKind;

    match (pending.kind.as_str(), control.response.as_ref()) {
        ("permission", Some(ControlResponseKind::Permission(decision))) => {
            let tool_name = pending
                .payload
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("the requested tool");
            if decision.allowed {
                Ok(format!(
                    "<system-reminder>User approved request {}. You may now proceed with {} if it is still the best next step.</system-reminder>",
                    pending.request_id, tool_name
                ))
            } else {
                Ok(format!(
                    "<system-reminder>User denied request {} for {}. Do not use that tool. Choose an alternative approach and explain the constraint.</system-reminder>",
                    pending.request_id, tool_name
                ))
            }
        }
        ("plan_review", Some(ControlResponseKind::PlanDecision(decision))) => match decision
            .action
            .as_str()
        {
            "approve" => Ok(
                "<system-reminder>User approved the proposed plan. Execute it now.</system-reminder>"
                    .to_string(),
            ),
            "modify" => Ok(plan_mode::format_plan_feedback(
                decision.feedback.as_deref().unwrap_or("Please revise the plan."),
            )),
            "reject" => Ok(
                "<system-reminder>User rejected the proposed plan. Do not execute it. Ask for clarification or propose a different approach.</system-reminder>"
                    .to_string(),
            ),
            other => Err(format!("unknown plan action '{other}'")),
        },
        ("permission", _) => Err("permission resume requires a permission decision".into()),
        ("plan_review", _) => Err("plan review resume requires a plan decision".into()),
        (other, _) => Err(format!("unsupported pending control kind '{other}'")),
    }
}
