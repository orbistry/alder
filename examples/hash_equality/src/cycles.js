export function expectCycle(operation, callback) {
    try {
        callback();
    } catch (error) {
        if (error instanceof TypeError && error.message === `${operation}: cyclic value`) return;
        throw error;
    }
    throw new Error(`${operation}: expected a cycle error`);
}
