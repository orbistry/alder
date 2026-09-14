use super::KERNEL_JS;

async fn run(source: &str) {
    let code = format!("{KERNEL_JS}\n{source}");
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        alder_runtime::execute(code, vec![]),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cloudflare_durable_object_initialization_storage_and_binding_isolation() {
    run(r#"
        class Base { constructor(ctx, env) {this.ctx=ctx;this.env=env;} }
        const storage = new Map();
        let initialized = 0;
        const ctx = {storage:{get:async key=>storage.get(key),put:async(key,value)=>storage.set(key,value)},blockConcurrencyWhile:callback=>callback()};
        const codec = {encode:JSON.stringify,decode:value=>$jsonDecodePrimitive(value,"number")};
        const dictionary = {
            initObject: state=>$task(function*(){initialized++;yield* $cloudflareStoragePut(codec,state,"count",7);return state;}),
            fetch: (state,request)=>$task(function*(){const value=yield* $cloudflareStorageGet(codec,state,"count");return new Response($providerGet("Cache")+":"+value._0);})
        };
        const Counter = $cloudflareDurableClass(Base,dictionary,[["Cache","CACHE"]]);
        const left = new Counter(ctx,{CACHE:"left"});
        const right = new Counter(ctx,{CACHE:"right"});
        const responses = await Promise.all([left.fetch(new Request("https://example.test")),right.fetch(new Request("https://example.test"))]);
        $assert(await responses[0].text() === "left:7");
        $assert(await responses[1].text() === "right:7");
        $assert(initialized===2);
        await left.fetch(new Request("https://example.test"));
        $assert(initialized===2);
        $assert(storage.get("count")==="7");
        let missing=false;try{await $cloudflareRun({},[["Cache","CACHE"]],()=>undefined);}catch(error){missing=error.message.includes("CACHE");}
        $assert(missing);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn cloudflare_queue_decodes_payloads_and_rejects_bad_batches() {
    run(r#"
        const seen=[];
        const handler=$cloudflareQueueHandler([["messages",{$super0:{decode:value=>$jsonDecodePrimitive(value,"number")},initConsumer:()=>0,consume:(consumer,messages)=>$task(function*(){seen.push([$providerGet("Cache"),messages]);})}]],[["Cache","CACHE"]]);
        await handler({queue:"messages",messages:[{body:1},{body:2}]},{CACHE:"bound"},{});
        $assert(JSON.stringify(seen)==='[["bound",[1,2]]]');
        let failed=false;try{await handler({queue:"messages",messages:[{body:"invalid"}]},{CACHE:"bound"},{});}catch(error){failed=error.message.includes("queue payload");}
        $assert(failed && seen.length===1);
        failed=false;try{await handler({queue:"unknown",messages:[]},{CACHE:"bound"},{});}catch(error){failed=error.message.includes("unknown");}
        $assert(failed);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn cloudflare_workflow_checkpoints_keep_codecs_and_provider_context() {
    run(r#"
        class Base {constructor(ctx,env){this.ctx=ctx;this.env=env;}}
        const codec={encode:JSON.stringify,decode:value=>$jsonDecodePrimitive(value,"number")};
        const stored=new Map();let executed=0;
        const step={do:async(name,callback)=>{if(!stored.has(name))stored.set(name,await callback());return stored.get(name);}};
        const dictionary={$super0:codec,$super1:codec,initWorkflow:()=>0,run:(workflow,event,step)=>$task(function*(){return yield* $cloudflareStep(codec,step,"calculate",()=>$task(function*(){executed++;return event.payload+$providerGet("Delta");}));})};
        const Job=$cloudflareWorkflowClass(Base,dictionary,[["Delta","DELTA"]]);
        const job=new Job({},{DELTA:2});
        $assert(await job.run({payload:40,instanceId:"one"},step)===42);
        $assert(await job.run({payload:40,instanceId:"one"},step)===42);
        $assert(executed===1 && stored.get("calculate")==="42");
    "#).await;
}
