// Adapters use the host classes passed by the generated Oxc module. All binding
// context is request/fiber-local; no environment object is cached globally.
export function $cloudflareHandleEq(left, right) {
    return left === right;
}

export function $cloudflareRun(env, providers, call) {
    return $runTask($cloudflareTask(env, providers, call));
}

export function $cloudflareTask(env, providers, call) {
    return $task(function* () {
        const pushed = [];
        try {
            for (const [key, binding] of providers) {
                if (!Object.hasOwn(env, binding)) throw new TypeError(`Missing Cloudflare binding ${binding}`);
                $providerPush(key, env[binding]);
                pushed.push(key);
            }
            const result = call();
            if (result?.[taskType]) return yield* result;
            if (result && typeof result.then === "function") return yield* $tryPromise(() => result);
            return result;
        } finally {
            for (const key of pushed.reverse()) $providerPop(key);
        }
    });
}

export function $cloudflareDurableClass(Base, dictionary, providers) {
    return class extends Base {
        constructor(ctx, env) {
            super(ctx, env);
            this.alderReady = this.ctx.blockConcurrencyWhile(async () => {
                this.alderValue = await $cloudflareRun(this.env, providers, () => dictionary.initObject(this.ctx));
            });
        }
        async fetch(request) {
            await this.alderReady;
            return await $cloudflareRun(this.env, providers, () => dictionary.fetch(this.alderValue, request));
        }
    };
}

function cloudflareDecode(dictionary, value, origin) {
    const decoded = dictionary.decode(JSON.stringify(value));
    if (decoded.$ !== "Ok") throw new TypeError(`${origin}: ${decoded._0._0}`);
    return decoded._0;
}

export function $cloudflareQueueHandler(consumers, providers) {
    const dictionaries = new Map(consumers);
    return async (batch, env, ctx) => {
        const dictionary = dictionaries.get(batch.queue);
        if (!dictionary) throw new TypeError(`No Alder consumer for queue ${batch.queue}`);
        // Returning successfully acknowledges the batch; a decode/task defect
        // rejects it so the platform applies its configured retry/dead-letter policy.
        await $cloudflareRun(env, providers, () => $task(function* () {
            const messages = batch.messages.map(message => cloudflareDecode(dictionary.$super0, message.body, "queue payload"));
            return yield* dictionary.consume(dictionary.initConsumer(), messages);
        }));
    };
}

export function $cloudflareWorkflowClass(Base, dictionary, providers) {
    return class extends Base {
        async run(event, step) {
            return await $cloudflareRun(this.env, providers, () => $task(function* () {
                const payload = cloudflareDecode(dictionary.$super0, event.payload, "workflow payload");
                const result = yield* dictionary.run(dictionary.initWorkflow(), {payload, instanceId: event.instanceId}, step);
                return JSON.parse(dictionary.$super1.encode(result));
            }));
        }
    };
}

export function $cloudflareKvGet(namespace, key) {
    return $tryPromise(async () => $resultOk(await namespace.get(key)), false,
        "cloudflare.kvGet", error => httpFailure(":cloudflare_error", error));
}

export function $cloudflareKvPut(namespace, key, value) {
    return $tryPromise(async () => { await namespace.put(key, value); return $resultOk(undefined); }, false,
        "cloudflare.kvPut", error => httpFailure(":cloudflare_error", error));
}

export function $cloudflareStorageGet(dictionary, state, key) {
    return $task(function* () {
        const stored = yield* $tryPromise(async () => $resultOk(await state.storage.get(key)), false,
            "cloudflare.storageGet", error => httpFailure(":cloudflare_error", error));
        if (stored.$ !== "Ok") return stored;
        if (stored._0 === undefined) return $resultOk(null);
        const decoded = dictionary.decode(stored._0);
        return decoded.$ === "Ok" ? $resultOk($optionSome(decoded._0)) : decoded;
    });
}

export function $cloudflareStoragePut(dictionary, state, key, value) {
    return $tryPromise(async () => { await state.storage.put(key, dictionary.encode(value)); return $resultOk(undefined); }, false,
        "cloudflare.storagePut", error => httpFailure(":cloudflare_error", error));
}

export function $cloudflareStep(dictionary, step, name, run) {
    return $task(function* () {
        const context = cloneContext(currentFiber.context);
        const encoded = yield* $tryPromise(() => step.do(name, async () => {
            const fiber = new FiberImpl(run(), null, context).start();
            const exit = await fiber.awaitExit();
            if (exit.$ !== "Success") throw exit.error;
            return dictionary.encode(exit.value);
        }), false, "cloudflare.step");
        const decoded = dictionary.decode(encoded);
        if (decoded.$ !== "Ok") throw new TypeError(`workflow checkpoint: ${decoded._0._0}`);
        return decoded._0;
    });
}
