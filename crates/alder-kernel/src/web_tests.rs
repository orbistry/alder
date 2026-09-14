use super::KERNEL_JS;

#[tokio::test(flavor = "current_thread")]
async fn request_payload_omits_server_only_stores_and_restores_event_only_shared_stores() {
    run(r#"
        const shared = $webStore('app/shared', 'count', () => 0, 'Number');
        const secret = $webStore('app/remote', 'secret', () => 'PRIVATE_STORE_SENTINEL', 'String');
        const route = {id:'/',segments:[],layouts:[],errors:[],options:[],page:{
          load:() => { $webStoreCell(secret); $webStoreCell(shared).value = 8; return {}; },
          page:() => $webComponent(() => render => { render.open('button'); render.event('click', () => $webStoreCell(shared).value++); render.text(() => 'safe', []); render.close(); }),
        }};
        const app = $webApplication({clientStoreKeys:['app/shared#count'],routes:[route]});
        const html = await (await app.fetch(new Request('http://localhost/'))).text();
        const payload = $webDecode(/<script type="application\/json" id="alder-data">(.*?)<\/script>/.exec(html)[1]);
        assert(!html.includes('PRIVATE_STORE_SENTINEL') && !html.includes('app/remote#secret'), 'private server stores never enter HTML hydration data');
        assert(payload.stores.length === 1 && payload.stores[0].value === 8, 'client allowlist preserves shared stores even when used only by event handlers');
        const data = await (await app.fetch(new Request('http://localhost/', {headers:{accept:'application/x-alder-data'}}))).text();
        assert(!data.includes('PRIVATE_STORE_SENTINEL') && $webDecode(data).stores.length === 1, 'navigation payload uses the same private-store boundary');
        $webRestoreStores(payload.stores);
        assert($webStoreCell(shared).value === 8, 'client restores the server-initialized shared store');
        const closed = $webApplication({routes:[route]});
        const closedData = await (await closed.fetch(new Request('http://localhost/', {headers:{accept:'application/x-alder-data'}}))).text();
        assert($webDecode(closedData).stores.length === 0, 'missing allowlist fails closed');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn module_stores_are_lazy_request_isolated_and_inherited_by_child_fibers() {
    run(r#"
        let initialized = 0;
        const store = $webStore('app/store', 'count', () => { initialized++; return 0; }, 'Number');
        const read = () => $webStoreCell(store).value;
        const write = value => { $webStoreCell(store).value = value; };
        assert(initialized === 0, 'declaring a store does not evaluate its initializer');
        let outside;
        try { read(); } catch (error) { outside = error; }
        assert(outside && initialized === 0, 'unscoped reads cannot initialize server-global state');
        const request = (initial, delay) => $runTask($webWithStoreScope(null, () => $task(function* () {
          write(initial);
          yield* $taskSleep(delay);
          const child = yield* $fiberFork($task(function* () { assert(read() === initial, 'fork inherits its request store'); write(read() + 1); }));
          yield* $fiberJoin(child);
          return {value: read(), stores: $webStoreSnapshot()};
        })));
        const [first, second] = await Promise.all([request(10, 4), request(20, 1)]);
        assert(first.value === 11 && second.value === 21 && initialized === 2, 'concurrent requests each initialize and mutate independent cells');
        assert(first.stores[0].value === 11 && second.stores[0].value === 21, 'request snapshots contain only their own store values');
        assert(!$webCurrentStoreScope(), 'request context leaves no singleton behind');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn asynchronous_ssr_preserves_request_providers_in_nested_setup_and_render_reads() {
    run(r#"
        const store = $webStore('app/store', 'service', () => $providerGet('service'), 'String');
        const nested = $webComponent(owner => {
          const name = $providerGet('service');
          const resource = $webResource(owner, [], () => $task(function* () {
            yield* $taskSleep(1);
            return $resultOk($providerGet('service') + ':' + $webStoreCell(store).value);
          }), 'nested');
          return render => {
            render.open('span');
            render.attr('title', () => $providerGet('service'), []);
            render.text(() => name + ':' + resource.value._0, [resource]);
            render.close();
          };
        }, 'Nested');
        const view = $webComponent(owner => {
          $webResource(owner, [], () => $task(function* () { yield* $taskSleep(1); return $resultOk(0); }), 'root');
          return render => render.value(() => nested, []);
        }, 'Root');
        const request = name => $runTask($webWithStoreScope(null, () => $task(function* () {
          $providerPush('service', name);
          try { return yield* $tryPromise(() => $webSsrAsync(view)); }
          finally { $providerPop('service'); }
        })));
        const [first, second] = await Promise.all([request('first'), request('second')]);
        assert(first.html.includes('first:first:first') && !first.html.includes('second'), 'first request retains providers through async nested rendering');
        assert(second.html.includes('second:second:second') && !second.html.includes('first'), 'second request has isolated providers and stores');
        assert(!$webCurrentStoreScope(), 'SSR does not leak ambient request state');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn synchronous_and_task_event_handlers_receive_the_native_event() {
    run(r#"
        let seen, completed;
        const view = $webComponent(owner => render => {
          render.open('button');
          render.event('click', event => { seen = event; });
          render.event('click', event => $task(function* () { yield* $taskSleep(1); completed = event; }));
          render.close();
        });
        const document = new TestDocument();
        const target = document.createElement('main');
        const instance = $webMount(view, target);
        const event = {type: 'click', clientX: 42};
        element(target, 'button').dispatchEvent(event);
        await $runTask($taskSleep(2));
        assert(seen === event && completed === event, 'both handler kinds receive the original browser event and Tasks are dispatched');
        const detached = element(target, 'button');
        instance.dispose();
        seen = null; completed = null;
        detached.dispatchEvent(event);
        await $runTask($taskSleep(2));
        assert(seen === null && completed === null, 'disposed event listeners cannot launch handlers');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn browser_owner_context_survives_events_memos_and_explicit_resource_refresh() {
    run(r#"
        let resource, count, observed;
        const view = $webComponent(owner => {
          count = $webState(owner, 0);
          const value = $webMemo(owner, [count], () => $providerGet('service') + count.value);
          resource = $webResource(owner, [], () => {
            const service = $providerGet('service');
            return $task(function* () { return $resultOk(service); });
          }, 'data');
          return render => {
            render.open('button');
            render.event('click', () => { observed = $providerGet('service'); count.value++; });
            render.text(() => value.value, [value]); render.close();
          };
        });
        const document = new TestDocument();
        const target = document.createElement('main');
        $providerPush('service', 'owner');
        const instance = $webMount(view, target);
        $providerPop('service');
        await resource.pending;
        element(target, 'button').dispatchEvent({type: 'click'});
        assert(observed === 'owner' && target.textContent === 'owner1', 'event and memo callbacks retain their owner provider context');
        $webResourceRefresh(resource);
        await resource.pending;
        assert(resource.value._0 === 'owner', 'explicit refresh restores context while constructing the loader task');
        instance.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn module_store_ssr_hydration_and_multiple_browser_instances_share_correct_cells() {
    run(r#"
        let initialized = 0;
        const store = $webStore('app/store', 'count', () => { initialized++; return 1; }, 'Number');
        const increment = () => { $webStoreCell(store).value++; };
        const view = $webComponent(owner => {
          const count = $webStoreCell(store);
          const doubled = $webMemo(owner, [count], () => count.value * 2);
          return render => { render.open('button'); render.event('click', increment); render.text(() => doubled.value, [doubled]); render.close(); };
        }, 'Counter');
        const scope = $webCreateStoreScope();
        $webStoreRun(scope, () => { $webStoreCell(store).value = 4; });
        const rendered = await $webSsrAsync(view, {storeScope: scope});
        $webDisposeStoreScope(scope);
        const document = new TestDocument();
        const target = parseSsr(document, rendered.html);
        const before = nodes(target);
        $webRestoreStores(rendered.stores);
        const first = $webHydrate(view, target);
        assert(initialized === 1 && target.textContent === '8' && before.every((node, index) => nodes(target)[index] === node), 'hydration primes serialized stores before setup without initializer reruns');
        const other = document.createElement('main');
        const second = $webMount(view, other);
        element(target, 'button').dispatchEvent({type: 'click'});
        assert(target.textContent === '10' && other.textContent === '10', 'browser components share a singleton store and settled memo scheduler');
        first.dispose();
        element(other, 'button').dispatchEvent({type: 'click'});
        assert(other.textContent === '12' && $webStoreSnapshot()[0].value === 6, 'unmounting one owner preserves the browser store');
        $webRestoreStores([...rendered.stores, {key: 'app/new#value', signature: 'Number', value: 9}], true);
        assert($webStoreCell(store).value === 6, 'navigation does not overwrite existing browser stores');
        const added = $webStore('app/new', 'value', () => 0, 'Number');
        assert($webStoreCell(added).value === 9, 'navigation primes previously unseen stores');
        second.dispose();
        assert($webStoreCell(store).subscribers.size === 0, 'component disposal detaches store subscriptions');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_resource_refresh_and_invalidation_bypass_query_caches() {
    run(r#"
        let resource, dependency;
        const forced = [];
        const view = $webComponent(owner => {
          dependency = $webState(owner, 0);
          resource = $webResource(owner, [dependency], () => $task(function* () {
            yield* $taskSleep(1);
            forced.push($webResourceTrack('app/query'));
            return $resultOk(dependency.value);
          }), 'query');
          return render => render.text(() => resource.value.$, [resource]);
        });
        const document = new TestDocument();
        const instance = $webMount(view, document.createElement('main'));
        await resource.pending;
        dependency.value++;
        await resource.pending;
        $webResourceRefresh(resource);
        await resource.pending;
        $webResourceInvalidate('app/query');
        await resource.pending;
        assert(forced.join() === 'false,false,true,true', 'only explicit refresh and invalidation force remote query cache bypass');
        assert($webResourceTrack('app/query') === false, 'ordinary nonresource query calls remain cacheable');
        instance.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn resources_track_query_modules_across_hydration_and_scope_local_invalidation() {
    run(r#"
        let requests = 0, resource;
        const view = $webComponent(owner => {
          resource = $webResource(owner, [], () => $task(function* () {
            requests++;
            yield* $taskSleep(1);
            $webResourceTrack('app/remote');
            $webResourceTrack('app/second');
            return $resultOk(requests);
          }), 'query');
          return render => render.text(() => resource.value.$ === 'Ready' ? resource.value._0 : 'loading', [resource]);
        }, 'Query');
        const result = await $webSsrAsync(view);
        assert(result.resources[0].queries[0] === 'app/remote', 'query context survives task suspension and is serialized');
        const document = new TestDocument();
        const target = parseSsr(document, result.html);
        const instance = $webHydrate(view, target, {resources: result.resources});
        assert(requests === 1, 'hydration reuses query results');
        $webResourceInvalidate('app/other');
        $webResourceInvalidate('app/remote', $webCreateStoreScope());
        assert(requests === 1, 'unrelated modules and request scopes do not refresh browser resources');
        $webResourceInvalidate('app/remote');
        await resource.pending;
        assert(requests === 2 && target.textContent === '2', 'matching invalidation refreshes hydrated resources');
        $webResourceInvalidate(null);
        await resource.pending;
        assert(requests === 3, 'all-module invalidation deduplicates resources tracking multiple modules');
        $webResourceCancel(resource);
        $webResourceInvalidate('app/remote');
        assert(requests === 3, 'cancellation removes query registrations');
        $webResourceRefresh(resource);
        await resource.pending;
        instance.dispose();
        $webResourceInvalidate('app/remote');
        assert(requests === 4 && $webCurrentStoreScope().queryResources.size === 0, 'disposal removes all query registrations');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn incompatible_hot_state_keeps_resource_hydration_payload_and_releases_partial_owners() {
    run(r#"
        let requests = 0, data;
        const view = signature => $webComponent(owner => {
          $webState(owner, 0, 'count');
          data = $webResource(owner, [], () => $task(function* () { requests++; return $resultOk('ready'); }), 'data');
          return render => render.text(() => data.value._0, [data]);
        }, 'View', signature);
        const result = await $webSsrAsync(view('new'));
        const document = new TestDocument();
        const previousTarget = document.createElement('main');
        const previous = $webMount(view('old'), previousTarget, {resources: result.resources});
        const state = previous.snapshot();
        previous.dispose();
        const target = parseSsr(document, result.html);
        const before = nodes(target);
        const instance = $webHydrate(view('new'), target, {state, resources: result.resources});
        assert(instance.restoration.status === 'incompatible' && requests === 1 && target.textContent === 'ready', 'fresh HMR fallback reuses resource payload without issuing requests');
        assert(before.every((node, index) => nodes(target)[index] === node), 'fallback hydration preserves existing DOM');
        instance.dispose();
        assert(data.subscribers.size === 0 && $webCurrentStoreScope().renderOwners.size === 0, 'failed and successful owners both release subscriptions');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn module_store_hot_updates_transfer_encoded_values_between_kernel_instances() {
    let next_kernel = KERNEL_JS.replace("export ", "");
    let harness = format!(
        "function nextKernel() {{ {next_kernel}\nreturn {{ $webStore, $webStoreCell, $webRestoreStores, $webDecode, $optionUnbox }}; }}\n{}",
        r#"
        let initialized = 0;
        $webRestoreStores([]);
        const original = $webStore('app/store', 'nested', () => { initialized++; return $optionSome(null); }, 'Option[Option[Number]]');
        const old = $webStoreCell(original).value;
        const encoded = $webEncode($webStoreSnapshot());
        const next = nextKernel();
        next.$webRestoreStores(next.$webDecode(encoded));
        const updated = next.$webStore('app/store', 'nested', () => { initialized++; return null; }, 'Option[Option[Number]]');
        const restored = next.$webStoreCell(updated).value;
        assert(initialized === 1 && restored !== old && next.$optionUnbox(restored) === null, 'encoded transfer reboxes Options for the new kernel and preserves compatible singleton state');
        const changed = next.$webStore('app/store', 'nested', () => 'fresh', 'String');
        assert(next.$webStoreCell(changed).value === 'fresh', 'incompatible store types initialize fresh values');
    "#
    );
    run(&harness).await;
}

#[tokio::test(flavor = "current_thread")]
async fn asynchronous_ssr_aborts_resources_and_hydration_requires_matching_payloads() {
    run(r#"
        let cleaned = 0, resource;
        const pending = $webComponent(owner => {
          resource = $webResource(owner, [], () => $task(function* () {
            try {
              yield* $tryPromise(signal => new Promise(resolve => signal.addEventListener('abort', resolve, {once: true})), true);
              return $resultOk(1);
            }
            finally { cleaned++; }
          }), 'data');
          return render => render.text(() => 'never', []);
        }, 'Pending');
        const controller = new AbortController();
        const rendered = $webSsrAsync(pending, {signal: controller.signal});
        await Promise.resolve();
        controller.abort();
        let aborted;
        try { await rendered; } catch (error) { aborted = error; }
        assert(aborted && cleaned === 1 && resource.subscribers.size === 0, 'aborted SSR waits for interrupted resource cleanup');
        let requests = 0;
        const ready = $webComponent(owner => {
          const value = $webResource(owner, [], () => $task(function* () { requests++; return $resultErr({$: ':missing'}); }), 'data');
          return render => render.text(() => value.value.$, [value]);
        }, 'Ready');
        const result = await $webSsrAsync(ready);
        const document = new TestDocument();
        const target = parseSsr(document, result.html);
        let mismatch;
        try { $webHydrate(ready, target); } catch (error) { mismatch = error; }
        assert(mismatch?.message.includes('missing resource') && requests === 1, 'missing hydration resources reject before refetching');
        const instance = $webHydrate(ready, target, {resources: result.resources});
        assert(target.textContent === 'Failed' && requests === 1, 'typed failed results also hydrate without refetch');
        instance.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn resources_suspend_ssr_once_hydrate_results_and_refresh_from_dependencies() {
    run(r#"
        let requests = 0, setups = 0, data, input;
        const nested = $webComponent(owner => {
          setups++;
          const value = $webResource(owner, [], () => $task(function* () {
            requests++; yield* $taskSleep(1); return $resultOk('nested');
          }), 'nested');
          return render => render.text(() => value.value._0, [value]);
        }, 'Nested', 'resource:nested');
        const view = $webComponent(owner => {
          setups++;
          input = $webState(owner, 1, 'input');
          data = $webResource(owner, [input], () => $task(function* () {
            requests++; const key = input.value; yield* $taskSleep(1); return $resultOk(key === 1 ? '<&>' : 'updated');
          }), 'data');
          return render => {
            render.open('section');
            render.text(() => data.value.$ === 'Ready' ? data.value._0 : 'loading', [data]);
            render.value(() => nested, []);
            render.close();
          };
        }, 'View', 'input|resource:data|Nested');
        const rendered = await $webSsrAsync(view);
        assert(setups === 2 && requests === 2 && rendered.resources.length === 2, 'async SSR executes parent and child setups/loaders once');
        assert(rendered.html.includes('&lt;&amp;&gt;') && !rendered.html.includes('loading'), 'SSR waits for values and escapes them');
        const document = new TestDocument();
        const target = parseSsr(document, rendered.html);
        const before = nodes(target);
        const allocated = document.allocations;
        const instance = $webHydrate(view, target, {resources: rendered.resources});
        assert(requests === 2 && setups === 4, 'hydration reconstructs setup but reuses serialized resource results');
        assert(document.allocations === allocated && before.every((node, index) => nodes(target)[index] === node), 'resource hydration reuses every node');
        input.value = 2;
        assert(target.textContent === 'loadingnested', 'dependency changes expose Loading immediately');
        await data.pending;
        assert(target.textContent === 'updatednested' && requests === 3, 'dependency refresh updates in place');
        $webResourceRefresh(data);
        await data.pending;
        assert(requests === 4 && target.textContent === 'updatednested', 'explicit refresh runs a new task');
        instance.dispose();
        assert(data.subscribers.size === 0 && input.subscribers.size === 0, 'resource ownership releases all dependency edges');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn resources_cancel_stale_fibers_and_preserve_typed_failures() {
    run(r#"
        let resource, input, aborted = 0, requests = 0;
        const view = $webComponent(owner => {
          input = $webState(owner, 0);
          resource = $webResource(owner, [input], () => $task(function* () {
            requests++;
            const value = input.value;
            yield* $tryPromise(signal => new Promise(resolve => {
              const timer = setTimeout(resolve, 20);
              signal.addEventListener('abort', () => { aborted++; clearTimeout(timer); resolve(); }, {once: true});
            }), true);
            return value === 2 ? $resultErr({$: ':missing', _0: value}) : $resultOk(value);
          }), 'data');
          return render => render.text(() => resource.value.$, [resource]);
        }, 'View');
        const document = new TestDocument();
        const target = document.createElement('main');
        const instance = $webMount(view, target);
        await Promise.resolve();
        input.value = 2;
        await resource.pending;
        assert(aborted === 1 && requests === 2 && resource.value.$ === 'Failed' && resource.value._0.$ === ':missing', 'superseded tasks cancel and Err remains a typed failure');
        $webResourceRefresh(resource);
        await Promise.resolve();
        $webResourceCancel(resource);
        await resource.pending;
        assert(aborted === 2 && resource.value.$ === 'Loading', 'explicit cancellation cannot publish a stale result');
        $webResourceRefresh(resource);
        await Promise.resolve();
        instance.dispose();
        await resource.pending;
        assert(aborted === 3 && target.childNodes.length === 0, 'owner disposal interrupts pending tasks');
        let syncError;
        try { $webSsr(view); } catch (error) { syncError = error; }
        assert(syncError?.message.includes('asynchronous SSR'), 'sync SSR cannot silently emit unresolved resources');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn reactive_handler_values_replace_callbacks_without_reattaching_listeners() {
    run(r#"
        let callback;
        const seen = [];
        const view = $webComponent(owner => {
          callback = $webState(owner, event => seen.push('first:' + event.clientX));
          return render => { render.open('button'); render.eventValue('click', () => callback.value, [callback]); render.close(); };
        });
        const document = new TestDocument();
        const target = parseSsr(document, $webSsr(view));
        const instance = $webHydrate(view, target);
        const button = element(target, 'button');
        button.dispatchEvent({type: 'click', clientX: 1});
        callback.value = event => seen.push('second:' + event.clientX);
        button.dispatchEvent({type: 'click', clientX: 2});
        assert(seen.join(',') === 'first:1,second:2' && listenerCount(target) === 1, 'callback inputs and native event payloads remain live');
        instance.dispose();
        assert(callback.subscribers.size === 0, 'handler dependencies are owned');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn restored_regions_do_not_resurrect_state_after_disposal() {
    run(r#"
        let shown;
        const view = $webComponent(owner => {
          shown = $webState(owner, true, 'shown');
          return render => render.region(() => shown.value ? ['shown', undefined, child => {
            const count = $webState(child, 0, 'count');
            return row => {
              row.open('button'); row.event('click', () => count.value++);
              row.text(() => count.value, [count]); row.close();
            };
          }] : undefined, [shown]);
        }, 'View', 'shown|region|count');
        const document = new TestDocument();
        const target = document.createElement('main');
        let instance = $webMount(view, target);
        element(target, 'button').dispatchEvent({type: 'click'});
        const state = instance.snapshot();
        instance.dispose();
        instance = $webMount(view, target, {state});
        assert(target.textContent === '1', 'initial restoration finds branch state');
        const removed = element(target, 'button');
        shown.value = false;
        assert(instance.snapshot().cells.size === 1, 'disposed child cells are excluded from later snapshots');
        shown.value = true;
        removed.dispatchEvent({type: 'click'});
        assert(target.textContent === '0', 'newly mounted branches start fresh and disposed listeners stay inert');
        instance.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn void_rcdata_and_template_content_follow_html_parser_shape() {
    run(r#"
        let value;
        const view = $webComponent(owner => {
          value = $webState(owner, '<&>');
          return render => {
            render.open('input'); render.attr('disabled', () => true, []); render.close();
            render.open('textarea'); render.text(() => 'prefix:', []); render.text(() => value.value, [value]); render.close();
            render.open('template'); render.open('span'); render.text(() => value.value, [value]); render.close(); render.close();
          };
        });
        const html = $webSsr(view);
        assert(html.startsWith('<input disabled=""><textarea>\nprefix:&lt;&amp;&gt;</textarea>'), 'void elements and RCDATA omit invalid markers and preserve the textarea newline rule');
        const document = new TestDocument();
        const target = parseSsr(document, html);
        const before = nodes(target);
        const allocations = document.allocations;
        const instance = $webHydrate(view, target);
        assert(document.allocations === allocations, 'RCDATA and template hydrate without allocating');
        const textarea = element(target, 'textarea');
        const text = textarea.firstChild;
        value.value = 'updated';
        assert(textarea.firstChild === text && textarea.textContent === 'prefix:updated', 'RCDATA segments update their shared text node');
        assert(element(target, 'template').content.textContent === 'updated', 'template children use content fragment');
        assert(before.every(node => nodes(target).includes(node)), 'special HTML element nodes remain stable');
        instance.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn textarea_leading_newlines_survive_sync_and_async_ssr_hydration() {
    run(r#"
        for (const initial of ['', '\nfirst', '\n\nfirst']) {
          const view = $webComponent(() => render => {
            render.open('textarea'); render.text(() => initial, []); render.close();
          });
          for (const html of [$webSsr(view), (await $webSsrAsync(view)).html]) {
            const document = new TestDocument();
            const target = parseSsr(document, html);
            const before = nodes(target);
            const instance = $webHydrate(view, target);
            assert(element(target, 'textarea').textContent === initial, 'HTML parser stripping preserves the user newline count');
            assert(before.every((node, index) => nodes(target)[index] === node), 'textarea hydration reuses parsed nodes');
            instance.dispose();
          }
        }
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn keyed_match_payload_updates_keep_current_handlers_and_release_removed_subtrees() {
    run(r#"
        let items, selected = [], branches = [];
        const view = $webComponent(owner => {
          items = $webState(owner, [{id: 1, kind: 'a', text: 'one'}, {id: 2, kind: 'a', text: 'two'}]);
          return render => render.list(() => items.value, [items], item => item.id, (child, item) => row => {
            row.region(() => [item.value.kind, item.value, (branch, payload) => {
              branches.push(payload);
              return body => {
                body.open('button');
                body.eventValue('click', () => { const text = payload.value.text; return () => selected.push(text); }, [payload]);
                body.text(() => payload.value.text, [payload]); body.close();
              };
            }], [item]);
          });
        });
        const document = new TestDocument();
        const target = document.createElement('main');
        const instance = $webMount(view, target);
        const original = nodes(target).filter(node => node.localName === 'button');
        items.value = [{id: 2, kind: 'a', text: 'TWO'}, {id: 1, kind: 'a', text: 'ONE'}];
        original[0].dispatchEvent({type: 'click'});
        assert(selected.join() === 'ONE' && branches.length === 2 && listenerCount(target) === 2, 'same match arm updates captured handler payload without rerunning setup or adding listeners');
        items.value = [{id: 1, kind: 'b', text: 'changed'}];
        original[0].dispatchEvent({type: 'click'}); original[1].dispatchEvent({type: 'click'});
        element(target, 'button').dispatchEvent({type: 'click'});
        assert(selected.join() === 'ONE,changed' && branches[0].subscribers.size === 0 && branches[1].subscribers.size === 0, 'changed match arms and removed keys detach old handlers and input subscriptions');
        instance.dispose();
        assert(items.subscribers.size === 0 && branches.every(cell => cell.subscribers.size === 0), 'nested keyed/match disposal leaves no reactive edges');
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn hot_updates_restore_only_compatible_live_writable_cells_before_memos() {
    run(r#"
        let count;
        function factory(version, signature = 'count:Number|keyed', initial = 0) {
          return $webComponent(owner => {
            count = $webState(owner, initial, 'count');
            const doubled = $webMemo(owner, [count], () => count.value * 2);
            return render => {
              render.text(() => version + ':' + doubled.value, [doubled]);
              render.list(() => version === 'old' ? [1, 2] : [2, 1], [], item => item, (child, item) => {
                const local = $webState(child, 10, 'local');
                return row => {
                  row.open('button');
                  row.attr('id', () => String(item.value), []);
                  row.event('click', () => local.value++);
                  row.text(() => local.value, [local]);
                  row.close();
                };
              });
            };
          }, 'app/View', signature);
        }
        const document = new TestDocument();
        const target = document.createElement('main');
        let instance = $webMount(factory('old'), target);
        count.value = 3;
        nodes(target).find(node => node.getAttribute?.('id') === '1').dispatchEvent({type: 'click'});
        const snapshot = $webSnapshot(instance);
        assert(snapshot.cells.size === 3, 'snapshots exclude derived and item input cells');
        instance.dispose();
        let disposedError;
        try { $webSnapshot(instance); } catch (error) { disposedError = error; }
        assert(disposedError, 'disposed instances cannot provide stale snapshots');
        instance = $webMount(factory('new'), target, {state: snapshot});
        assert(instance.restoration.status === 'restored' && target.textContent === 'new:61011', 'restore precedes memo evaluation and keyed state follows identity across reorder');
        instance.dispose();
        instance = $webMount(factory('new', 'incompatible'), target, {state: snapshot});
        assert(instance.restoration.status === 'incompatible' && target.textContent === 'new:01010', 'signature mismatch falls back atomically to fresh state');
        instance.dispose();
        instance = $webMount(factory('new', 'count:Number|keyed', 'changed'), target, {state: snapshot});
        assert(instance.restoration.status === 'incompatible' && count.value === 'changed', 'runtime shape mismatch cannot partially restore a component');
        instance.dispose();
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn keyed_regions_hydrate_reorder_refresh_and_dispose() {
    run(r#"
        let items, visible;
        let setups = 0;
        const view = $webComponent(owner => {
          items = $webState(owner, [{id: 1, text: 'a'}, {id: 2, text: 'b'}]);
          visible = $webState(owner, true);
          return render => render.region(() => visible.value ? ['yes', undefined, () => nested => {
            nested.list(() => items.value, [items], item => item.id, (child, item) => {
              setups++;
              return row => {
                row.open('button');
                row.event('click', () => {});
                row.text(() => item.value.text, [item]);
                row.close();
              };
            }, () => row => row.text(() => 'empty', []));
          }] : undefined, [visible]);
        });
        const document = new TestDocument();
        const target = parseSsr(document, $webSsr(view));
        const before = nodes(target);
        const allocations = document.allocations;
        const mounted = $webHydrate(view, target);
        assert(document.allocations === allocations && before.every((node, index) => nodes(target)[index] === node), 'region hydration preserves every node');
        const buttons = nodes(target).filter(node => node.localName === 'button');
        const beforeSetups = setups;
        items.value = [{id: 2, text: 'B'}, {id: 1, text: 'A'}];
        assert(setups === beforeSetups && target.textContent === 'BA', 'keys retain setup and update item inputs');
        assert(nodes(target).filter(node => node.localName === 'button')[0] === buttons[1], 'key reorder moves existing nodes');
        items.value = [];
        assert(target.textContent === 'empty' && listenerCount(target) === 0, 'removed keys dispose listeners and render empty');
        visible.value = false;
        assert(target.textContent === '' && items.subscribers.size === 0, 'branch disposal releases list dependency');
        mounted.dispose();
        assert(target.childNodes.length === 0, 'all region anchors are owned');
    "#).await;
}

async fn run(harness: &str) {
    let shim = include_str!("../../../tests/support/dom-shim.js").replace("export ", "");
    let code = format!("{KERNEL_JS}\n{shim}\n{harness}");
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        alder_runtime::execute(code, vec![]),
    )
    .await
    .expect("web kernel regression terminates")
    .expect("web kernel assertions pass");
}

#[tokio::test(flavor = "current_thread")]
async fn memo_dependencies_are_explicit_deduplicated_and_disposable() {
    run(r#"
        const owner = webOwner();
        const count = $webState(owner, 1);
        const other = $webState(owner, 9);
        let runs = 0;
        const doubled = $webMemo(owner, [count, count], () => { runs++; return count.value * 2; });
        assert(runs === 1 && doubled.value === 2, "memo initial computation");
        other.value = 10;
        count.value = 1;
        assert(runs === 1, "unrelated and equal writes do not recompute");
        count.value = 2;
        assert(runs === 2 && doubled.value === 4, "one computation per dependency change");
        assert(count.subscribers.size === 1, "duplicate dependencies share a subscription");
        webDispose(owner);
        webDispose(owner);
        assert(count.subscribers.size === 0, "disposal removes memo subscriptions");
        count.value = 3;
        assert(runs === 2 && count.value === 2, "disposed writes are inert");
    "#)
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn diamond_dependencies_settle_before_dom_effects() {
    run(r#"
        const owner = webOwner();
        const count = $webState(owner, 1);
        const left = $webMemo(owner, [count], () => count.value + 1);
        const right = $webMemo(owner, [count], () => count.value * 2);
        const total = $webMemo(owner, [left, right, count], () => left.value + right.value + count.value);
        const observed = [];
        webSubscribe(owner, [count, left, right, total], () => observed.push(total.value));
        count.value = 2;
        assert(observed.length === 1 && observed[0] === 9, "effects observe settled values exactly once");
        webDispose(owner);
        assert([count, left, right, total].every(cell => cell.subscribers.size === 0), "all edges released");
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn empty_text_boundaries_hydrate_without_allocation_and_update_in_place() {
    run(r#"
        let state;
        const view = $webComponent(owner => {
          state = $webState(owner, "");
          return render => {
            render.text(() => state.value, [state]);
            render.text(() => "next", []);
          };
        });
        const html = $webSsr(view);
        assert(html === "<!--alder:text--><!--/alder:text--><!--alder:text-->next<!--/alder:text-->", "adjacent and empty boundaries serialize");
        const document = new TestDocument();
        const target = parseSsr(document, html);
        const before = nodes(target);
        const allocations = document.allocations;
        const instance = $webHydrate(view, target);
        assert(document.allocations === allocations, "empty hydration allocates no text node");
        state.value = "now";
        assert(target.textContent === "nownext", "empty hole inserts its first text between anchors");
        const text = target.childNodes[1];
        state.value = "later";
        assert(text === target.childNodes[1] && target.textContent === "laternext", "subsequent updates preserve text identity");
        state.value = "";
        assert(target.textContent === "next" && text === target.childNodes[1], "clearing preserves identity");
        assert(before.every(node => nodes(target).includes(node)), "original hydration nodes remain");
        instance.dispose();
        assert(target.childNodes.length === 0 && state.subscribers.size === 0, "root insertions and subscriptions are owned");
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn failed_ssr_mount_and_hydration_release_partial_owners() {
    run(r#"
        let state;
        const broken = $webComponent(owner => {
          state = $webState(owner, 1);
          $webMemo(owner, [state], () => state.value * 2);
          return render => {
            render.open("button");
            render.event("click", () => { state.value++; });
            throw new Error("intentional render failure");
          };
        });
        const document = new TestDocument();
        for (const mode of ["ssr", "mount", "hydrate"]) {
          const target = parseSsr(document, mode === "hydrate" ? "<button></button>" : "<span></span>");
          const existing = target.firstChild;
          let error;
          try {
            if (mode === "ssr") $webSsr(broken);
            else if (mode === "mount") $webMount(broken, target);
            else $webHydrate(broken, target);
          } catch (caught) { error = caught; }
          assert(error?.message === "intentional render failure", "original error preserved");
          assert(state.subscribers.size === 0, "failure removes partial subscriptions");
          assert(target.childNodes.length === 1 && target.firstChild === existing, "failure preserves pre-existing DOM");
          assert(listenerCount(target) === 0, "failure removes partial event listeners");
        }
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn hydration_rejects_structural_attribute_and_namespace_mismatches() {
    run(r#"
        const view = $webComponent(() => render => {
          render.open("button");
          render.attr("title", () => "expected", []);
          render.event("click", () => {});
          render.text(() => "text", []);
          render.close();
        });
        const document = new TestDocument();
        const html = $webSsr(view);
        for (const corrupt of [
          target => { target.firstChild.localName = "span"; },
          target => { target.firstChild.namespaceURI = "http://www.w3.org/2000/svg"; },
          target => { target.firstChild.setAttribute("title", "wrong"); },
          target => { target.firstChild.setAttribute("extra", "unexpected"); },
          target => { target.firstChild.firstChild.data = "bad marker"; },
          target => { target.firstChild.appendChild(document.createTextNode("extra")); },
          target => { target.appendChild(document.createElement("span")); },
        ]) {
          const target = parseSsr(document, html);
          corrupt(target);
          const before = nodes(target);
          const mutations = document.mutations;
          let error;
          try { $webHydrate(view, target); } catch (caught) { error = caught; }
          assert(error?.message.startsWith("Alder hydration mismatch:"), "explicit mismatch error");
          assert(document.mutations === mutations && before.every((node, index) => nodes(target)[index] === node), "no mismatch repair");
          assert(listenerCount(target) === 0, "mismatch rolls back listeners");
        }
    "#).await;
}
