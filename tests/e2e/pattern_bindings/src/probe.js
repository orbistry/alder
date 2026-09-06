import { $runTask } from "alder:kernel";

export async function runChecked(task, cleanup, expectedCount) {
    try {
        await $runTask(task);
        return false;
    } catch (error) {
        return error.message.includes("Non-exhaustive match at alder://")
            && cleanup.length === expectedCount
            && cleanup.every(value => value === 99);
    }
}
