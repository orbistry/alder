use super::KERNEL_JS;

#[tokio::test(flavor = "current_thread")]
async fn navigation_preloads_selected_static_graph_before_imports_without_duplicates() {
    run_navigation(r#"
      document.head=document.createElement("head");
      document.querySelectorAll=()=>document.head.childNodes;
      const urls=()=>Array.from(document.head.childNodes,node=>node.getAttribute("href"));
      const text=value=>$webComponent(()=>r=>r.text(()=>value,[]));
      const preloads={normal:["/_alder/entry.mjs","/_alder/shared.mjs","/_alder/other.mjs"],errors:[["/_alder/entry.mjs","/_alder/error.mjs"]]};
      const routes=[
        {id:"/initial",segments:[["static","initial"]],page:{page:()=>text("initial")},layouts:[],errors:[]},
        {id:"/other",segments:[["static","other"]],lazy:true,layouts:[],errors:[async()=>{
          $assert(urls().includes("/_alder/error.mjs"));return {error:()=>text("error")};
        }],page:async()=>{
          $assert(JSON.stringify(urls())===JSON.stringify(preloads.normal));
          return {page:()=>text("other")};
        }},
      ];
      const bootstrap={build:"build",route:"/initial",data:{},params:{},error:null,boundary:-1,options:{ssr:false,csr:true},stores:[],resources:[]};
      const session=await $webStartLazyClient({routes});
      let boundary=-1;
      globalThis.fetch=async()=>new Response($webEncode({...bootstrap,route:"/other",preloads,boundary,serverData:[{}]}),{headers:{"content-type":"application/x-alder-data"}});
      await session.navigate("http://localhost/other");
      await session.navigate("http://localhost/other");
      $assert(urls().length===3 && target.textContent==="other");
      boundary=0;
      await session.navigate("http://localhost/other");
      $assert(urls().length===4 && target.textContent==="error");
      session.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn deployment_mismatch_recovers_once_without_rendering_mixed_build_data() {
    run_navigation(r#"
      document.body=document.createElement("body");
      const redirects=[],storage=new Map();
      globalThis.sessionStorage={getItem:key=>storage.get(key),setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)};
      location.assign=href=>redirects.push(href);
      const text=value=>$webComponent(()=>r=>r.text(()=>value,[]));
      const route={id:"/initial",segments:[["static","initial"]],lazy:true,page:{page:()=>text("old")},layouts:[],errors:[]};
      const bootstrap={build:"old-build",route:"/initial",data:{},params:{},error:null,boundary:-1,options:{ssr:false,csr:true},stores:[],resources:[]};
      const session=await $webStartLazyClient({routes:[route]});
      document.querySelectorAll=()=>{throw new Error("Must not preload a different build");};
      globalThis.fetch=async()=>new Response($webEncode({...bootstrap,build:"new-build",preloads:{normal:["/_alder/new-build.mjs"]}}),{headers:{"content-type":"application/x-alder-data"}});
      const log=console.error;console.error=()=>{};
      try {
        await session.navigate(location.href);
        await session.navigate(location.href);
        $assert(redirects.length===1 && target.textContent==="old" && historyEntries.length===0);
        $assert(document.body.textContent.includes("Check your connection") && document.body.textContent.includes("Reload page"));
      } finally {console.error=log;session.dispose();}
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn mismatched_initial_entry_rejects_before_loading_or_hydrating() {
    run_navigation(r#"
      let loaded=0;
      const bootstrap={entry:"/_alder/new.mjs",route:"/initial"};
      let rejected=false;
      try {await $webStartLazyClient({entryUrl:"http://localhost/_alder/old.mjs",routes:[{id:"/initial",lazy:true,layouts:[],page:async()=>{loaded++;return {};}}]});}
      catch(error){rejected=error.message.includes("different builds");}
      $assert(rejected && loaded===0 && target.childNodes.length===0);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn missing_lazy_chunk_recovers_once_and_storage_denial_never_reloads() {
    run_navigation(r#"
      document.body=document.createElement("body");
      const redirects=[],storage=new Map();
      globalThis.sessionStorage={getItem:key=>storage.get(key),setItem:(key,value)=>storage.set(key,value),removeItem:key=>storage.delete(key)};
      location.assign=href=>redirects.push(href);
      const routes=[
        {id:"/initial",segments:[["static","initial"]],page:{page:()=>$webComponent(()=>r=>r.text(()=>"old",[]))},layouts:[],errors:[]},
        {id:"/missing",segments:[["static","missing"]],lazy:true,page:async()=>{throw new TypeError("Failed to fetch dynamically imported module");},layouts:[],errors:[]},
      ];
      const bootstrap={build:"build",route:"/initial",data:{},params:{},error:null,boundary:-1,options:{ssr:false,csr:true},stores:[],resources:[]};
      const session=await $webStartLazyClient({routes});
      globalThis.fetch=async()=>new Response($webEncode({...bootstrap,route:"/missing"}),{headers:{"content-type":"application/x-alder-data"}});
      const log=console.error;console.error=()=>{};
      try {
        await session.navigate("http://localhost/missing");
        await session.navigate("http://localhost/missing");
        $assert(redirects.length===1 && target.textContent==="old" && historyEntries.length===0);
        globalThis.sessionStorage={getItem(){throw new Error("denied");}};
        webRecover("http://localhost/another",new Error("offline"));
        $assert(redirects.length===1 && document.body.textContent.includes("Reload page"));
      } finally {console.error=log;session.dispose();}
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn production_documents_preload_only_selected_graph_and_assets_have_safe_cache_headers() {
    run(r#"
      const client={entry:"/_alder/entry-123.mjs",build:"build-1",files:["/_alder/entry-123.mjs","/_alder/chunk-456.mjs"],routes:{"/":{normal:["/_alder/entry-123.mjs","/_alder/chunk-456.mjs"],errors:[]}}};
      const route={id:"/",segments:[],page:{page:()=>$webComponent(()=>r=>r.text(()=>"home",[]))},layouts:[],errors:[],options:[]};
      const app=$webApplication({routes:[route],client});
      const response=await app.fetch(new Request("http://localhost/"));
      const html=await response.text();
      $assert(html.includes('import("/_alder/entry-123.mjs")') && html.includes('rel="modulepreload" href="/_alder/chunk-456.mjs"'));
      $assert(!html.includes('/_alder/client.mjs') && response.headers.get("cache-control")==="no-store");
      const payload=$webDecode(await(await app.fetch(new Request("http://localhost/",{headers:{accept:"application/x-alder-data"}}))).text());
      $assert(payload.build==="build-1" && payload.entry===client.entry);
      $assert(JSON.stringify(payload.preloads.normal)===JSON.stringify(client.routes["/"].normal));
      $assert(payload.preloads.errors.length===0);
      const worker=$webWorker(app,"",undefined,[[client.entry,"text/javascript",[65,66]]]);
      const get=await worker.fetch(new Request("http://localhost"+client.entry),{},{});
      $assert(await get.text()==="AB" && get.headers.get("cache-control").includes("immutable"));
      const head=await worker.fetch(new Request("http://localhost"+client.entry,{method:"HEAD"}),{},{});
      $assert(await head.text()==="" && head.headers.get("content-length")==="2");
      let fallback=0;
      const env={ASSETS:{fetch:async()=>{fallback++;return new Response("js",{headers:{"content-type":"text/javascript"}});}}};
      const remote=await worker.fetch(new Request("http://localhost/_alder/chunk-456.mjs"),env,{});
      $assert(await remote.text()==="js" && remote.headers.get("cache-control").includes("immutable") && fallback===1);
      for(const path of ["/_alder/removed.mjs","/_alder/chunk-456.mjs.map","/_alder/client.mjs"]) {
        const missing=await worker.fetch(new Request("http://localhost"+path),env,{});
        $assert(missing.status===404 && missing.headers.get("cache-control")==="no-store" && fallback===1);
      }
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn lazy_route_startup_hydrates_only_active_modules_and_navigation_loads_on_demand() {
    run_navigation(r#"
      const calls=[];
      const text=value=>$webComponent(()=>r=>r.text(()=>value,[]));
      const page={page:()=>text("initial")}, other={page:()=>text("other"),load:()=>{calls.push("load");return {};}};
      const lazy=(name,value)=>async()=>{calls.push(name);return value;};
      const routes=[
        {id:"/initial",segments:[["static","initial"]],page:lazy("initial",page),layouts:[],errors:[lazy("error",{error:()=>text("error")})],lazy:true},
        {id:"/other",segments:[["static","other"]],page:lazy("other",other),layouts:[],errors:[],lazy:true},
      ];
      const bootstrap={route:"/initial",data:{},params:{},error:null,boundary:-1,options:{ssr:true,csr:true},stores:[],resources:[]};
      const parsed=parseSsr(document,(await $webSsrAsync(page.page())).html);
      while(parsed.firstChild)target.appendChild(parsed.firstChild);
      const original=nodes(target);
      const session=await $webStartLazyClient({routes});
      $assert(calls.join(",")==="initial");
      $assert(original.every((node,index)=>nodes(target)[index]===node));
      globalThis.fetch=async()=>new Response($webEncode({...bootstrap,route:"/other",serverData:[{}]}),{headers:{"content-type":"application/x-alder-data"}});
      await session.navigate("http://localhost/other");
      $assert(calls.join(",")==="initial,other,load" && target.textContent==="other");
      session.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn superseded_lazy_import_does_not_mount_or_change_history() {
    run_navigation(r#"
      const text=value=>$webComponent(()=>r=>r.text(()=>value,[]));
      let resolveSlow,started;
      const waiting=new Promise(resolve=>started=resolve);
      const routes=[
        {id:"/initial",segments:[["static","initial"]],page:{page:()=>text("initial")},layouts:[],errors:[]},
        {id:"/slow",segments:[["static","slow"]],page:()=>{started();return new Promise(resolve=>resolveSlow=resolve);},layouts:[],errors:[],lazy:true},
        {id:"/fast",segments:[["static","fast"]],page:async()=>({page:()=>text("fast")}),layouts:[],errors:[],lazy:true},
      ];
      const bootstrap={route:"/initial",data:{},params:{},error:null,boundary:-1,options:{ssr:false,csr:true},stores:[],resources:[]};
      globalThis.fetch=async href=>new Response($webEncode({...bootstrap,route:new URL(href).pathname,serverData:[{}]}),{headers:{"content-type":"application/x-alder-data"}});
      const session=await $webStartLazyClient({routes});
      const slow=session.navigate("http://localhost/slow");
      await waiting;
      await session.navigate("http://localhost/fast");
      resolveSlow({page:()=>text("slow")});
      await slow;
      $assert(target.textContent==="fast" && historyEntries.length===1 && historyEntries[0].endsWith("/fast"));
      session.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn nested_layout_failures_recover_at_ancestor_boundaries_in_ssr_and_navigation() {
    run_navigation(r#"
      let mode="initial";
      const text=value=>$webComponent(()=>r=>r.text(()=>value,[]));
      const outer={layout:props=>$webComponent(()=>r=>{r.text(()=>"outer:",[]);r.value(()=>props.children,[]);})};
      const inner={load:()=>{if(mode==="load")throw new Error("inner load");return {};},layout:props=>$webComponent(()=>{
        if(mode==="setup")throw new Error("inner setup");
        return r=>{r.text(()=>"inner:",[]);r.value(()=>props.children,[]);};
      })};
      const page={page:()=>$webComponent(()=>{if(mode!=="initial")throw new Error("page");return r=>r.text(()=>"page",[]);})};
      const errors=[{error:()=>text("ancestor")},{error:()=>{if(mode==="boundary")throw new Error("nearest failed");return text("nearest");}}];
      const server={load:()=>{if(mode==="server-load")throw new Error("inner server load");return {};}};
      const route={id:"/[id]",segments:[["param","id"]],layouts:[[outer,undefined],[inner,server]],errorLayouts:[1,2],errors,page,options:[]};
      const application=$webApplication({routes:[route]});
      const bootstrap=$webDecode(await(await application.fetch(new Request("http://localhost/initial",{headers:{accept:"application/x-alder-data"}}))).text());
      bootstrap.options.ssr=false;
      globalThis.fetch=(href,init)=>application.fetch(new Request(href,init));
      const session=$webStartClient({routes:[route],hook:{handleError:()=>{}}});
      for(const scenario of ["page","load","server-load","setup","boundary"]){
        mode=scenario;
        const response=await application.fetch(new Request("http://localhost/"+scenario));
        const html=await response.text();
        $assert(response.status===500);
        $assert(html.includes(scenario==="page" ? "nearest" : "ancestor"));
        const payload=$webDecode(await(await application.fetch(new Request("http://localhost/"+scenario,{headers:{accept:"application/x-alder-data"}}))).text());
        $assert(payload.boundary===(scenario==="page" ? 1 : 0));
        await session.navigate("http://localhost/"+scenario);
        $assert(target.textContent===(scenario==="page" ? "outer:inner:nearest" : "outer:ancestor"));
      }
      session.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn endpoint_action_remote_hooks_intercept_fetch_and_report_each_defect_once() {
    run(r#"
      const previousFetch=globalThis.fetch;
      globalThis.fetch=async()=>new Response("upstream");
      let fail=false, handles=0, intercepted=0;
      const reports=[];
      const read=()=>$task(function*(){
        const result=yield* $httpFetch(new Request("http://upstream/value"));
        $assert(result.$==="Ok");
        const text=yield* $tryPromise(()=>result._0.text());
        if(fail)throw new Error("private defect");
        return text;
      });
      const descriptor={args:value=>value,result:value=>value,call:read};
      const app=$webApplication({routes:[
        {id:"/endpoint",segments:[["static","endpoint"]],endpoint:{get:()=>$task(function*(){return new Response(yield* read());})}},
        {id:"/action",segments:[["static","action"]],page:{page:()=>null},layouts:[],errors:[],options:[]},
      ],remotes:[{...descriptor,module:"app:remote",name:"read",kind:"query"}],actions:[{...descriptor,route:"/action",name:"read"}],hook:{
        handle:(event,resolve)=>$task(function*(){handles++;$providerPush("session",event.url.pathname);try{return yield* resolve(event);}finally{$providerPop("session");}}),
        handleFetch:(event,request,next)=>$task(function*(){intercepted++;yield* $taskSleep(1);$assert($providerGet("session")===event.url.pathname);return yield* next(request);}),
        handleError:(error,event)=>{reports.push([event.url.pathname,error.message]);},
      }});
      const requests=()=>[
        new Request("http://localhost/endpoint"),
        new Request("http://localhost/action",{method:"POST",headers:{"content-type":"application/x-alder-value","x-alder-remote":"1","x-alder-route":"%2Faction","x-alder-action":"read"},body:$webEncode([])}),
        new Request("http://localhost/_alder/remote/app%3Aremote/read",{method:"POST",headers:{"content-type":"application/x-alder-value","x-alder-remote":"1"},body:$webEncode([])}),
      ];
      try {
        for(const request of requests()) {const response=await app.fetch(request);$assert(response.status===200 && (await response.text()).includes("upstream"));}
        fail=true;
        for(const request of requests()) {const response=await app.fetch(request);$assert(response.status===500 && await response.text()==="Internal server error");}
        $assert(handles===6 && intercepted===6 && reports.length===3 && new Set(reports.map(value=>value[0])).size===3);
        $assert(reports.every(value=>value[1]==="private defect") && synchronousProviderContext.size===0);
      } finally {globalThis.fetch=previousFetch;}
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn client_resource_defects_report_but_expected_failures_remain_values() {
    let shim = include_str!("../../../tests/support/dom-shim.js").replace("export ", "");
    let harness = r#"
      const document = globalThis.document = new TestDocument();
      const target = document.createElement("main");
      const payload = {route:"/", data:{}, params:{}, resources:[], stores:[], options:{ssr:false, csr:true}};
      document.getElementById = id => id === "alder-app" ? target : {textContent:$webEncode(payload)};
      document.addEventListener = document.removeEventListener = () => {};
      globalThis.window = {addEventListener(){},removeEventListener(){},scrollTo(){}};
      globalThis.location = {href:"http://localhost/",origin:"http://localhost",pathname:"/",search:""};
      let defect=true, resource;
      const reports=[];
      const route = {id:"/",segments:[],layouts:[],errors:[],page:{page:()=>$webComponent(owner=>{
        resource=$webResource(owner,[],()=>$task(function*(){yield* $taskSleep(1);if(defect)throw new Error("resource defect");return $resultErr({$:":missing"});}),"data");
        return r=>r.text(()=>"ready",[]);
      })}};
      const config={routes:[route],hook:{handleError:error=>{reports.push(error);}}};
      const first=$webStartClient(config);
      for(let attempt=0;attempt<100 && reports.length<1;attempt++) await new Promise(resolve=>setTimeout(resolve,1));
      $assert(reports.length===1 && reports[0].message==="resource defect");
      first.dispose();defect=false;
      const second=$webStartClient(config);
      for(let attempt=0;attempt<100 && resource.value.$==="Loading";attempt++) await new Promise(resolve=>setTimeout(resolve,1));
      $assert(resource.value.$==="Failed" && resource.value._0.$===":missing" && reports.length===1);
      second.dispose();
    "#;
    run(&format!("{shim}\n{harness}")).await;
}

#[tokio::test(flavor = "current_thread")]
async fn client_event_and_startup_defects_use_owner_scoped_error_hooks() {
    let shim = include_str!("../../../tests/support/dom-shim.js").replace("export ", "");
    let harness = r#"
      const document = globalThis.document = new TestDocument();
      const target = document.createElement("main");
      const payload = {route:"/", data:{}, params:{}, resources:[], stores:[], options:{ssr:false, csr:true}};
      document.getElementById = id => id === "alder-app" ? target : {textContent:$webEncode(payload)};
      document.addEventListener = document.removeEventListener = () => {};
      globalThis.window = {addEventListener(){},removeEventListener(){},scrollTo(){}};
      globalThis.location = {href:"http://localhost/",origin:"http://localhost",pathname:"/",search:""};
      const oldReports=[], newReports=[];
      let mode="sync", release;
      const route = {id:"/",segments:[],layouts:[],errors:[],page:{page:()=>$webComponent(()=>r=>{
        r.open("button"); r.event("click",()=>{
          if(mode==="sync") throw new Error("sync event");
          return $task(function*(){
            if(mode==="late") yield* $tryPromise(()=>new Promise(resolve=>{release=resolve;}));
            else yield* $taskSleep(1);
            throw new Error("task event");
          });
        });r.text(()=>"click",[]);r.close();
      })}};
      const old=$webStartClient({routes:[route],hook:{handleError:error=>{oldReports.push(error);}}});
      target.firstChild.dispatchEvent({type:"click"});
      mode="task"; target.firstChild.dispatchEvent({type:"click"});
      for(let attempt=0;attempt<100 && oldReports.length<2;attempt++) await new Promise(resolve=>setTimeout(resolve,1));
      $assert(oldReports.length===2 && oldReports[0].message==="sync event" && oldReports[1].message==="task event");
      mode="late";target.firstChild.dispatchEvent({type:"click"});
      for(let attempt=0;attempt<100 && !release;attempt++) await new Promise(resolve=>setTimeout(resolve,1));
      $assert(typeof release==="function");
      old.dispose();
      const next=$webStartClient({routes:[route],hook:{handleError:error=>{newReports.push(error);}}});
      release();await new Promise(resolve=>setTimeout(resolve,5));
      $assert(oldReports.length===2 && newReports.length===0);
      mode="sync";target.firstChild.dispatchEvent({type:"click"});
      await new Promise(resolve=>setTimeout(resolve,1));
      $assert(newReports.length===1 && newReports[0].message==="sync event");
      next.dispose();
      const badReports=[];
      let threw=false;
      try {$webStartClient({routes:[{...route,page:{page:()=>$webComponent(()=>{throw new Error("startup failed");})}}],hook:{handleError:error=>{badReports.push(error);}}});}
      catch(error){threw=error.message==="startup failed";}
      await new Promise(resolve=>setTimeout(resolve,1));
      $assert(threw && badReports.length===1 && badReports[0].message==="startup failed" && target.childNodes.length===0);
    "#;
    run(&format!("{shim}\n{harness}")).await;
}

#[tokio::test(flavor = "current_thread")]
async fn client_init_and_navigation_failures_reach_the_typed_reporting_hook() {
    let shim = include_str!("../../../tests/support/dom-shim.js").replace("export ", "");
    let harness = r#"
      const document = globalThis.document = new TestDocument();
      const target = document.createElement("main");
      const payload = {route:"/", data:{}, params:{}, resources:[], stores:[], options:{ssr:false, csr:true}};
      document.getElementById = id => id === "alder-app" ? target : {textContent:$webEncode(payload)};
      const listeners = new Map();
      document.addEventListener = (name, callback) => listeners.set(name, callback);
      document.removeEventListener = name => listeners.delete(name);
      globalThis.window = {addEventListener:document.addEventListener, removeEventListener:document.removeEventListener, scrollTo(){}};
      globalThis.location = {href:"http://localhost/", origin:"http://localhost", pathname:"/", search:""};
      globalThis.history = {pushState(){}};
      const route = {id:"/", segments:[], layouts:[], errors:[], page:{page:() => $webComponent(() => r => r.text(() => "ready", []))}};
      const reports = [];
      let initialized = 0;
      const session = $webStartClient({routes:[route], hook:{
        init:() => $task(function*() {initialized++; yield* $taskSleep(1); throw new Error("init failed");}),
        handleError:error => $task(function*() {yield* $taskSleep(1); reports.push(error);}),
      }});
      const settled = async count => {
        for (let attempt=0; attempt<100 && reports.length<count; attempt++) await new Promise(resolve=>setTimeout(resolve,1));
        $assert(reports.length===count);
      };
      await settled(1);
      $assert(initialized===1 && reports[0].name==="Error" && reports[0].message==="init failed" && typeof reports[0].stack==="string");
      globalThis.fetch = async () => {throw new Error("navigation failed");};
      listeners.get("popstate")();
      await settled(2);
      $assert(initialized===1 && reports[1].message==="navigation failed" && target.textContent==="ready");
      globalThis.fetch = async () => {const error=new Error("cancelled");error.name="AbortError";throw error;};
      listeners.get("popstate")();
      await new Promise(resolve=>setTimeout(resolve,5));
      $assert(reports.length===2);
      session.dispose();
      $assert(listeners.size===0 && target.childNodes.length===0);
      let syncInit=0;
      const sync = $webStartClient({routes:[route], hook:{init:()=>{syncInit++;}}});
      await new Promise(resolve=>setTimeout(resolve,1));
      $assert(syncInit===1);
      sync.dispose();
    "#;
    run(&format!("{shim}\n{harness}")).await;
}

#[tokio::test(flavor = "current_thread")]
async fn public_assets_preserve_binary_bytes_head_and_query_paths() {
    run(r#"
      let calls = 0;
      const application = {fetch:async () => {calls++; return new Response("route", {status:404});}};
      const worker = $webWorker(application, "client source", undefined, [
        ["/image.png", "image/png", [0, 255, 128, 10]],
        ["/hello world.txt", "text/plain; charset=utf-8", [65, 66]],
      ]);
      const binary = await worker.fetch(new Request("http://localhost/image.png?version=2"));
      $assert(binary.status === 200 && binary.headers.get("content-type") === "image/png");
      $assert(binary.headers.get("content-length") === "4" && binary.headers.get("x-content-type-options") === "nosniff");
      $assert(Array.from(new Uint8Array(await binary.arrayBuffer())).join(",") === "0,255,128,10");
      const head = await worker.fetch(new Request("http://localhost/image.png", {method:"HEAD"}));
      $assert(head.status === 200 && head.headers.get("content-length") === "4" && await head.text() === "");
      const encoded = await worker.fetch(new Request("http://localhost/hello%20world.txt"));
      $assert(await encoded.text() === "AB");
      const post = await worker.fetch(new Request("http://localhost/image.png", {method:"POST"}));
      $assert(post.status === 405 && post.headers.get("allow") === "GET, HEAD" && calls === 0);
      $assert((await worker.fetch(new Request("http://localhost/missing"))).status === 404 && calls === 1);
      $assert(await (await worker.fetch(new Request("http://localhost/_alder/client.mjs?revision=3"))).text() === "client source");
      const dev = $webWorker(application, "client", undefined, [], false);
      const stale = {ASSETS:{fetch:async () => new Response("old build")}};
      $assert((await dev.fetch(new Request("http://localhost/removed"), stale)).status === 404);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn client_failed_navigation_and_hot_setup_keep_the_previous_tree_live() {
    let shim = include_str!("../../../tests/support/dom-shim.js").replace("export ", "");
    let harness = r#"
      const document = globalThis.document = new TestDocument();
      const target = document.createElement("main");
      const payload = {route:"/", data:{}, params:{}, resources:[], stores:[], options:{ssr:false, csr:true}};
      document.getElementById = id => id === "alder-app" ? target : {textContent:$webEncode(payload)};
      const listeners = new Map();
      document.addEventListener = (name, callback) => listeners.set(callback, name);
      document.removeEventListener = (name, callback) => listeners.delete(callback);
      globalThis.window = {addEventListener:document.addEventListener, removeEventListener:document.removeEventListener, scrollTo(){}};
      globalThis.location = {href:"http://localhost/", origin:"http://localhost", pathname:"/", search:""};
      globalThis.history = {pushState(){}};
      globalThis.EventSource = class {addEventListener(){} close(){}};
      let clicks = 0;
      const good = {id:"/", segments:[], layouts:[], errors:[], page:{page:() => $webComponent(() => r => {
        r.open("button"); r.event("click", () => {clicks++;}); r.text(() => "live", []); r.close();
      })}};
      const bad = {id:"/bad", segments:[["static","bad"]], layouts:[], errors:[], page:{page:() => $webComponent(() => r => {
        r.open("span"); r.text(() => "partial", []); throw new Error("setup failed");
      })}};
      const session = $webStartClient({development:true, routes:[good,bad]});
      const original = target.firstChild;
      globalThis.fetch = async () => new Response($webEncode({...payload, route:"/bad"}), {headers:{"content-type":"application/x-alder-data"}});
      let failed = false;
      try {await session.navigate("http://localhost/bad");} catch(error) {failed = error.message === "setup failed";}
      $assert(failed && target.firstChild === original && target.textContent === "live");
      original.dispatchEvent({type:"click"});
      $assert(clicks === 1 && listeners.size === 2);
      globalThis[Symbol.for("alder.dev.transfer")] = {payload:$webEncode({...payload, route:"/bad"}), revision:2};
      failed = false;
      try {$webStartClient({development:true, routes:[good,bad]});} catch(error) {failed = error.message === "setup failed";}
      $assert(failed && target.firstChild === original && target.textContent === "live");
      original.dispatchEvent({type:"click"});
      $assert(clicks === 2 && listeners.size === 2);
      session.dispose();
      $assert(target.childNodes.length === 0 && listeners.size === 0);
    "#;
    run(&format!("{shim}\n{harness}")).await;
}

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
async fn application_ssr_loads_layouts_and_serializes_client_data() {
    run(r#"
      const calls = [];
      const page = {load: event => {calls.push("page"); return {message: event.data.parent + event.params.id};},
        page: props => $webComponent(() => r => {r.open("h1"); r.text(() => props.data.message, []); r.close();})};
      const layout = {load: () => {calls.push("layout"); return {parent: "Hello "};},
        layout: props => $webComponent(() => r => {r.open("main"); r.value(() => props.children); r.close();})};
      const route = {id: "/[id]", segments: [["param", "id"]], page, layouts: [[layout, undefined]], errors: [], options: []};
      const app = $webApplication({routes: [route]});
      const response = await app.fetch(new Request("http://localhost/alder"));
      const html = await response.text();
      $assert(response.status === 200 && html.includes("<main>") && html.includes("Hello alder"));
      $assert(response.headers.get("cache-control") === "no-store");
      $assert(html.includes('src="/_alder/client.mjs"') && html.includes('id="alder-data"'));
      $assert(calls.join(",") === "layout,page");
      const data = await app.fetch(new Request("http://localhost/next", {headers:{accept:"application/x-alder-data"}}));
      const payload = $webDecode(await data.text());
      $assert(data.headers.get("cache-control") === "no-store" && data.headers.get("vary").includes("x-alder-navigation"));
      $assert(payload.data.message === "Hello next" && payload.params.id === "next");
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn server_hooks_keep_request_context_through_suspending_loads() {
    run(r#"
      const page = {load: () => $task(function*() {
        const first = $providerGet("session");
        yield* $taskSleep(2);
        $assert(first === $providerGet("session"));
        return {session: first};
      }), page: props => $webComponent(() => r => r.text(() => props.data.session, []))};
      const hook = {handle: (event, resolve) => $task(function*() {
        $providerPush("session", event.params.id);
        try {return yield* resolve(event);} finally {$providerPop("session");}
      })};
      const app = $webApplication({hook, routes:[{id:"/[id]", segments:[["param","id"]], page, layouts:[], errors:[], options:[]}]});
      const [one, two] = await Promise.all(["one","two"].map(async id => {
        const response = await app.fetch(new Request("http://localhost/"+id, {headers:{accept:"application/x-alder-data"}}));
        return $webDecode(await response.text()).data.session;
      }));
      $assert(one === "one" && two === "two");
      $assert(synchronousProviderContext.size === 0);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn http_endpoints_options_and_invalid_paths_have_explicit_behavior() {
    run(r#"
      const endpoint = {get: () => new Response("healthy", {headers:{"content-type":"text/plain"}})};
      const page = {page: () => $webComponent(() => r => r.text(() => "static", [])), csr:false, trailingSlash:{$:"Always"}};
      const app = $webApplication({routes:[
        {id:"/api", segments:[["static","api"]], endpoint, layouts:[], errors:[], options:[]},
        {id:"/page", segments:[["static","page"]], page, layouts:[], errors:[], options:[page]},
      ]});
      $assert(await (await app.fetch(new Request("http://localhost/api"))).text() === "healthy");
      $assert((await app.fetch(new Request("http://localhost/api", {method:"POST"}))).status === 405);
      $assert((await app.fetch(new Request("http://localhost/missing"))).status === 404);
      $assert((await app.fetch(new Request("http://localhost/%zz"))).status === 400);
      const redirect = await app.fetch(new Request("http://localhost/page?x=1"));
      $assert(redirect.status === 308 && redirect.headers.get("location") === "http://localhost/page/?x=1");
      const html = await (await app.fetch(new Request("http://localhost/page/"))).text();
      $assert(html.includes("static") && !html.includes("<script"));
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn remotes_validate_requests_and_results_inside_request_hooks() {
    run(r#"
      let calls = 0, hooks = 0;
      const descriptor = {module:"app:users", name:"get", kind:"query",
        args: value => {if (!Array.isArray(value) || value.length !== 1 || typeof value[0] !== "string") throw new TypeError(); return value;},
        result: value => {if (typeof value !== "string") throw new TypeError(); return value;},
        call: name => $task(function*() {calls++; yield* $taskSleep(1); return $providerGet("session") + name;})};
      const app = $webApplication({routes:[], remotes:[descriptor], hook:{handle:(event, resolve) => $task(function*() {
        hooks++; $providerPush("session", "user:");
        try {return yield* resolve(event);} finally {$providerPop("session");}
      }), handleError:() => {}}});
      const invoke = (body, headers = {}, method = "POST") => app.fetch(new Request("http://localhost/_alder/remote/app%3Ausers/get", {
        method, ...(method === "POST" ? {body} : {}), headers:{"content-type":"application/x-alder-value", "x-alder-remote":"1", ...headers},
      }));
      let response = await invoke($webEncode(["alice"]), {origin:"http://localhost"});
      $assert(response.status === 200 && $webDecode(await response.text()) === "user:alice");
      $assert(response.headers.get("cache-control") === "no-store");
      $assert((await invoke($webEncode(["alice"]), {origin:"https://evil.example"})).status === 403);
      $assert((await invoke($webEncode(["alice"]), {"x-alder-remote":""})).status === 403);
      $assert((await invoke($webEncode([12]))).status === 400);
      $assert((await invoke("invalid")).status === 400);
      const oversized = await invoke("x".repeat(1048577));
      if (oversized.status !== 413) throw new Error(`oversized status ${oversized.status}`);
      $assert((await invoke("", {}, "GET")).status === 405);
      $assert(calls === 1 && hooks === 7);
      descriptor.call = () => 12;
      $assert((await invoke($webEncode(["alice"]))).status === 500);
      $assert(synchronousProviderContext.size === 0);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn remote_queries_cache_by_arguments_and_commands_invalidate_module() {
    run(r#"
      let requests = 0;
      const originalFetch = globalThis.fetch;
      globalThis.fetch = async (url, options) => {
        requests++;
        $assert(options.method === "POST" && options.credentials === "same-origin" && options.signal instanceof AbortSignal);
        return new Response($webEncode({request:requests, args:$webDecode(options.body)}), {headers:{"content-type":"application/x-alder-value"}});
      };
      try {
        const one = await $runTask($webRemote("users", "get", [1], "query"));
        one.args[0] = 99;
        const cached = await $runTask($webRemote("users", "get", [1], "query"));
        $assert(cached.request === 1 && cached.args[0] === 1 && requests === 1);
        await $runTask($webRemote("users", "get", [2], "query"));
        await $runTask($webRemote("other", "get", [1], "query"));
        await $runTask($webRemote("users", "save", [1], "command"));
        await $runTask($webRemote("other", "get", [1], "query"));
        $assert(requests === 4);
        await $runTask($webRemote("users", "get", [1], "query"));
        $assert(requests === 5);
        webResetRemoteCache();
        await $runTask($webRemote("users", "get", [1], "query"));
        $assert(requests === 6);
      } finally {globalThis.fetch = originalFetch;}
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn navigation_cache_reset_rejects_late_queries_from_the_previous_page() {
    run(r#"
      let release, requests = 0;
      globalThis.fetch = async () => {
        const request = ++requests;
        if (request === 1) await new Promise(resolve => {release = resolve;});
        return new Response($webEncode(request), {headers:{"content-type":"application/x-alder-value"}});
      };
      const old = $runTask($webRemote("users", "get", [1], "query"));
      while (!release) await Promise.resolve();
      webResetRemoteCache();
      release();
      $assert(await old === 1);
      $assert(await $runTask($webRemote("users", "get", [1], "query")) === 2);
      $assert(await $runTask($webRemote("users", "get", [1], "query")) === 2);
      $assert(requests === 2);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn action_dispatch_retains_route_url_params_and_typed_result_errors() {
    run(r#"
      const visited = [];
      const app = $webApplication({routes:[{id:"/users/[id]",segments:[["static","users"],["param","id"]]}],
        hook:{handle:(event,resolve) => $task(function*(){
          visited.push(event.url.pathname); $providerPush("id",event.params.id);
          try{return yield* resolve(event);}finally{$providerPop("id");}
        })}, actions:[{route:"/users/[id]",name:"save",
          args:value => {if(value.length !== 1 || typeof value[0].name !== "string")throw new TypeError();return value;},
          result:value => value, call:input => $task(function*(){yield* $taskSleep(1);return input.name ? $resultOk($providerGet("id")+":"+input.name) : $resultErr({$:":required"});}),
        }]});
      const invoke=(input,route="/users/[id]",name="save") => app.fetch(new Request("http://localhost/users/alder?x=1",{method:"POST",
        headers:{"content-type":"application/x-alder-value","x-alder-remote":"1","x-alder-route":encodeURIComponent(route),"x-alder-action":name},body:$webEncode([input])}));
      let response=await invoke({name:"Ada"});
      $assert(response.status===200 && $webDecode(await response.text())._0==="alder:Ada");
      response=await invoke({name:""});
      $assert(response.status===200 && $webDecode(await response.text())._0.$===":required");
      $assert((await invoke({name:"Ada"},"/other")).status===404);
      $assert((await invoke({name:1})).status===400);
      $assert(visited.length===4 && visited.every(path=>path==="/users/alder"));
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn fetch_hooks_intercept_std_fetch_and_preserve_request_context_and_result() {
    run(r#"
      const originalFetch=globalThis.fetch; let intercepted=0,native=0;
      globalThis.fetch=async request=>{native++;return new Response(request.url);};
      const app=$webApplication({hook:{handleFetch:(event,request,next)=>$task(function*(){
        intercepted++;yield* $taskSleep(1);$assert(event.params.id==="x");return yield* next(request);
      })},routes:[{id:"/[id]",segments:[["param","id"]],layouts:[],errors:[],options:[],
        page:{load:event=>$task(function*(){const result=yield* $httpFetch(new Request("http://upstream/test"));$assert(result.$==="Ok");return {text:yield* $tryPromise(()=>result._0.text())};}),page:()=>$webComponent(()=>()=>{})},
      }]});
      try {
        const response=await app.fetch(new Request("http://localhost/x",{headers:{accept:"application/x-alder-data"}}));
        $assert($webDecode(await response.text()).data.text==="http://upstream/test" && intercepted===1 && native===1);
        globalThis.fetch=async()=>{throw new Error("offline");};
        const result=await $runTask($httpFetch(new Request("http://upstream/")));
        $assert(result.$==="Err" && result._0.$===":network_error" && intercepted===1);
      } finally {globalThis.fetch=originalFetch;}
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn application_stores_are_isolated_across_suspension_and_serialized_after_ssr() {
    run(r#"
      let initializes=0;
      const handle=$webStore("app","count",()=>{initializes++;return 0;},"Number");
      const app=$webApplication({clientStoreKeys:["app#count"],routes:[{id:"/[id]",segments:[["param","id"]],layouts:[],errors:[],options:[],
        page:{load:event=>$task(function*(){const cell=$webStoreCell(handle);cell.value=Number(event.params.id);yield* $taskSleep(2);return {value:cell.value};}),
          page:props=>$webComponent(()=>r=>r.text(()=>$webStoreCell(handle).value,[]))},
      }]});
      const replies=await Promise.all(["1","2"].map(async id=>{
        const text=await(await app.fetch(new Request("http://localhost/"+id))).text();
        const data=/<script type="application\/json" id="alder-data">(.*?)<\/script>/.exec(text)[1];
        return $webDecode(data);
      }));
      $assert(initializes===2 && replies[0].data.value===1 && replies[1].data.value===2);
      $assert(replies[0].stores[0].value===1 && replies[1].stores[0].value===2);
      $assert(synchronousProviderContext.size===0);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn aborted_http_request_cancels_tasks_and_disposes_request_scope() {
    run(r#"
      const abort=new AbortController();let entered,closed=false,scope;
      const started=new Promise(resolve=>{entered=resolve;});
      const app=$webApplication({routes:[{id:"/",segments:[],endpoint:{get:()=>$task(function*(){
        scope=$webCurrentStoreScope();entered();try{yield* $taskSleep(60000);return new Response("late");}finally{closed=true;}
      })}}]});
      const result=app.fetch(new Request("http://localhost/",{signal:abort.signal}));
      await started;abort.abort();const response=await result;
      $assert(response.status===499 && closed && scope.closed && synchronousProviderContext.size===0);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn error_boundaries_preserve_expected_payloads_and_hide_unexpected_secrets() {
    run(r#"
      let reports=0,defect=false;
      const route={id:"/",segments:[],layouts:[],options:[],
        page:{load:()=>{if(defect)throw new Error("private secret");return $resultErr({$:":missing",_0:"user"});},page:()=>null},
        errors:[{error:props=>$webComponent(()=>r=>r.text(()=>props.error.$==="Expected" ? props.error._0._0 : props.error._0.message,[]))}],
      };
      const app=$webApplication({routes:[route],hook:{handleError:error=>{reports++;$assert(error.name==="Error" && error.message==="private secret" && typeof error.stack==="string");}}});
      let response=await app.fetch(new Request("http://localhost/"));
      let text=await response.text();$assert(response.status===500 && text.includes("user") && reports===0);
      defect=true;response=await app.fetch(new Request("http://localhost/"));text=await response.text();
      $assert(response.status===500 && text.includes("Internal server error") && !text.includes("private secret") && reports===1);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn prerender_evaluates_entries_once_and_serves_built_html_and_navigation_data() {
    run(r#"
      let loads=0,hooks=0;
      const page={prerender:true,load:event=>{loads++;return {id:event.params.id};},page:props=>$webComponent(()=>r=>r.text(()=>props.data.id,[]))};
      const route={id:"/[id]",segments:[["param","id"]],page,layouts:[],errors:[],options:[page]};
      const config={routes:[route],hook:{handle:(event,resolve)=>{hooks++;return resolve(event);}}};
      const app=$webApplication(config);
      const pages=await $webPrerender(app,[{route:"/[id]",segments:route.segments,trailingSlash:"Never",paths:[],
        entries:()=>$task(function*(){yield* $taskSleep(1);return [{id:"one"},{id:"two"}];}),validate:value=>value}]);
      $assert(loads===2 && pages.length===2 && pages[0].path==="/one");
      const built=$webApplication({...config,prerendered:pages});
      const html=await(await built.fetch(new Request("http://localhost/one"))).text();
      const data=await(await built.fetch(new Request("http://localhost/two",{headers:{accept:"application/x-alder-data"}}))).text();
      $assert(html===pages[0].html && $webDecode(data).data.id==="two" && loads===2 && hooks===4);
      let rejected=false;
      try {await $webPrerender(app,[{route:"/[id]",segments:route.segments,paths:["/one","/one"]}]);}catch{rejected=true;}
      $assert(rejected);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn prerender_worker_forwards_queue_consumer_and_build_environment() {
    run(r#"
      const env={binding:"build"},ctx={waitUntil(){}},batch={messages:[{body:"input"}]};
      let queueCalls=0,entryCalls=0;
      const queue=async (received,bindings,context)=>{
        $assert(received===batch && bindings===env && context===ctx);
        queueCalls++;
      };
      const application={entries:async (callback,bindings)=>{
        $assert(bindings===env);entryCalls++;return callback();
      }};
      const worker=$webPrerenderWorker(application,[{route:"/[id]",segments:[["param","id"]],paths:[],entries:()=>[],validate:value=>value}],queue);
      $assert(worker.queue===queue);
      await worker.queue(batch,env,ctx);
      const response=await worker.fetch(new Request("http://localhost/"),env,ctx);
      $assert(response.status===200 && await response.text()==="[]" && entryCalls===1 && queueCalls===1);
      $assert(!Object.hasOwn($webPrerenderWorker(application,[]),"queue"));
    "#).await;
}

async fn run_navigation(source: &str) {
    let shim = include_str!("../../../tests/support/dom-shim.js").replace("export ", "");
    let setup = r#"
      const document=globalThis.document=new TestDocument();
      const target=document.createElement("main"),listeners=new Map(),historyEntries=[];
      document.getElementById=id=>id==="alder-app" ? target : {textContent:$webEncode(bootstrap)};
      document.addEventListener=(name,callback)=>listeners.set(callback,name);
      document.removeEventListener=(name,callback)=>listeners.delete(callback);
      globalThis.window={addEventListener:document.addEventListener,removeEventListener:document.removeEventListener,scrollTo(){}};
      globalThis.location={href:"http://localhost/initial",origin:"http://localhost",pathname:"/initial",search:""};
      globalThis.history={pushState:(_state,_title,href)=>historyEntries.push(href)};
    "#;
    run(&format!("{shim}\n{setup}\n{source}")).await;
}

#[tokio::test(flavor = "current_thread")]
async fn browser_navigation_replays_universal_loads_with_trusted_server_stages() {
    run_navigation(r#"
      const calls=[],render=props=>$webComponent(()=>r=>r.text(()=>props.data.message,[]));
      const serverLayout={load:event=>{calls.push("server-universal-layout");return {parent:event.data.base+":server",shadow:"overwritten"};}};
      const serverPage={load:event=>{calls.push("server-universal-page");return {message:event.data.child+":server-page"};},page:render};
      const route={id:"/[id]",segments:[["param","id"]],layouts:[[serverLayout,{load:()=>({base:"base",shadow:"public-return"})}]],
        page:serverPage,server:{load:event=>{
          calls.push("server-page");$assert(event.data.parent==="base:server");
          return {child:event.data.parent+":"+$providerGet("request"),base:"child-base"};
        }},errors:[],options:[]};
      const config={routes:[route],hook:{handle:(event,resolve)=>$task(function*(){
        $providerPush("request",event.params.id);
        try{return yield* resolve(event);}finally{$providerPop("request");}
      })}};
      let application=$webApplication(config);
      const bootstrap=$webDecode(await(await application.fetch(new Request("http://localhost/initial",{headers:{accept:"application/x-alder-data"}}))).text());
      const browserRoute={...route,layouts:[[{load:event=>{
        calls.push("browser-layout");$assert(event.data.base==="base");return {parent:"browser-parent"};
      }},undefined]],server:undefined,page:{page:render,load:event=>$task(function*(){
        calls.push("browser-page");yield* $taskSleep(1);
        $assert(event.data.base==="child-base" && event.data.parent==="browser-parent");
        $assert(event.request.url===event.url.href && event.params.id===event.url.pathname.slice(1));
        return {message:event.data.parent+"|"+event.data.child};
      })}};
      let navigationPayload;
      globalThis.fetch=async (href,init)=>{
        $assert(init.headers["x-alder-navigation"]==="1");
        const response=await application.fetch(new Request(href,init));
        navigationPayload=$webDecode(await response.clone().text());
        return response;
      };
      const parsed=parseSsr(document,(await $webSsrAsync(render({data:bootstrap.data}))).html);
      while(parsed.firstChild)target.appendChild(parsed.firstChild);
      const original=nodes(target);
      const session=$webStartClient({routes:[browserRoute]});
      $assert(original.every((node,index)=>nodes(target)[index]===node));
      $assert(!calls.includes("browser-page") && target.textContent==="base:server:initial:server-page");
      calls.length=0;
      await session.navigate("http://localhost/next");
      $assert(calls.join(",")==="server-universal-layout,server-page,browser-layout,browser-page");
      $assert(target.textContent==="browser-parent|base:server:next");
      $assert(navigationPayload.serverData.length===2 && navigationPayload.serverData[0].shadow==="public-return");
      $assert(!Object.hasOwn(navigationPayload.serverData[0],"parent") && !Object.hasOwn(navigationPayload,"env"));
      const pages=await $webPrerender(application,[{route:"/[id]",segments:route.segments,paths:["/cached"]}]);
      application=$webApplication({...config,prerendered:pages});
      calls.length=0;
      await session.navigate("http://localhost/cached");
      $assert(calls.join(",")==="browser-layout,browser-page");
      $assert(target.textContent==="browser-parent|base:server:cached");
      $assert(historyEntries.join(",")==="http://localhost/next,http://localhost/cached");
      session.dispose();$assert(target.childNodes.length===0 && listeners.size===0);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn full_data_hmr_requests_keep_ssr_resources_but_navigation_defers_them() {
    run(r#"
      let loads=0,resources=0;
      const page={load:()=>{loads++;return {message:"loaded"};},page:props=>$webComponent(owner=>{
        const resource=$webResource(owner,[],()=>$task(function*(){resources++;yield* $taskSleep(1);return $resultOk("ready");}),"data");
        return render=>render.text(()=>resource.value.$==="Ready" ? resource.value._0 : "loading",[resource]);
      },"Page")};
      const application=$webApplication({routes:[{id:"/",segments:[],layouts:[],errors:[],options:[],page}]});
      const request=headers=>application.fetch(new Request("http://localhost/",{headers:{accept:"application/x-alder-data",...headers}}));
      const full=$webDecode(await(await request({})).text());
      $assert(loads===1 && resources===1 && full.data.message==="loaded" && full.resources.length===1);
      const navigation=$webDecode(await(await request({"x-alder-navigation":"1"})).text());
      $assert(loads===1 && resources===1 && navigation.resources.length===0 && navigation.serverData.length===1);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn browser_universal_load_failures_and_superseded_tasks_have_owned_cleanup() {
    run_navigation(r#"
      let cleanups=0,startedResolve,serverUniversals=0;
      const started=new Promise(resolve=>startedResolve=resolve),reports=[];
      const render=props=>$webComponent(()=>r=>r.text(()=>props.data.message,[]));
      const boundary={error:props=>$webComponent(()=>r=>r.text(()=>props.error.$==="Expected" ? props.error._0._0 : props.error._0.message,[]))};
      const route={id:"/[id]",segments:[["param","id"]],layouts:[],errors:[boundary],options:[],
        page:{page:render,load:()=>{serverUniversals++;return {message:"initial"};}}};
      const application=$webApplication({routes:[route]});
      const bootstrap=$webDecode(await(await application.fetch(new Request("http://localhost/initial",{headers:{accept:"application/x-alder-data"}}))).text());
      bootstrap.options.ssr=false;
      const browserRoute={...route,page:{page:render,load:event=>$task(function*(){
        if(event.params.id==="expected")return $resultErr({$:":invalid",_0:"browser-typed-error"});
        if(event.params.id==="unexpected")throw new Error("browser defect");
        if(event.params.id==="slow"){
          startedResolve();
          try {yield* $taskSleep(1000);} finally {cleanups++;}
        }
        return {message:event.params.id};
      })}};
      globalThis.fetch=(href,init)=>application.fetch(new Request(href,init));
      const session=$webStartClient({routes:[browserRoute],hook:{handleError:error=>reports.push(error)}});
      await session.navigate("http://localhost/expected");
      $assert(target.textContent==="browser-typed-error" && reports.length===0);
      await session.navigate("http://localhost/unexpected");
      $assert(target.textContent==="Internal server error" && reports.length===1 && reports[0].message==="browser defect");
      const slow=session.navigate("http://localhost/slow");
      await started;
      await session.navigate("http://localhost/fast");
      await slow;
      $assert(cleanups===1 && target.textContent==="fast" && serverUniversals===1);
      $assert(!historyEntries.includes("http://localhost/slow") && reports.length===1);
      const pending=session.navigate("http://localhost/slow");
      await new Promise(resolve=>setTimeout(resolve,5));
      session.dispose();await pending;
      $assert(cleanups===2 && target.childNodes.length===0 && listeners.size===0 && reports.length===1);
    "#).await;
}
