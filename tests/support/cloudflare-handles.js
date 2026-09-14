export function checkHandleIdentity(sameState, sameStep) {
    const state = { storage: {} };
    const step = { do() {} };
    if (!sameState(state, state) || sameState(state, { storage: state.storage })) {
        throw new Error("DurableObjectState equality must compare handle identity");
    }
    if (!sameStep(step, step) || sameStep(step, { do: step.do })) {
        throw new Error("WorkflowStep equality must compare handle identity");
    }
}
