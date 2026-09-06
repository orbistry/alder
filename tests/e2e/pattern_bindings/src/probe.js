import { $runTask } from "alder:kernel";

export function missingPayload() {
    throw new Error("Missing payload");
}

export async function runChecked(task, cleanup, expectedCount) {
    try {
        await $runTask(task);
        return false;
    } catch (error) {
        return error.message.includes("Missing payload")
            && cleanup.length === expectedCount
            && cleanup.every(value => value === 99);
    }
}
