use super::KERNEL_JS;

async fn run(source: &str) {
    let code = format!("{KERNEL_JS}\n{source}");
    assert_eq!(alder_runtime::execute(code, vec![]).await.unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn hydration_transport_preserves_alder_values_aliases_and_cycles() {
    run(r#"
      const option = $optionSome($optionSome(null));
      const original = { option, enum: {$: "Some", _0: null}, result: $resultErr({$: "missing"}),
        map: new Map(), set: new Set([42n]), date: new Date("2026-01-02T00:00:00Z"),
        unit: undefined, nan: NaN, negativeZero: -0, infinity: Infinity };
      original.self = original;
      original.map.set(original, option);
      const decoded = $webDecode($webEncode(original));
      $assert(decoded.self === decoded);
      $assert(decoded.map.get(decoded) === decoded.option);
      $assert($optionUnbox($optionUnbox(decoded.option)) === null);
      $assert($optionUnbox(decoded.enum) === decoded.enum);
      $assert(decoded.result.$ === "Err" && decoded.result._0.$ === "missing");
      $assert(decoded.set.has(42n) && decoded.date.toISOString() === original.date.toISOString());
      $assert(decoded.unit === undefined && Number.isNaN(decoded.nan));
      $assert(Object.is(decoded.negativeZero, -0) && decoded.infinity === Infinity);
    "#)
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn hydration_transport_is_deterministic_script_safe_and_prototype_safe() {
    run(r#"
      const payload = JSON.parse('{"__proto__":{"polluted":true},"constructor":"safe","text":"</script>&"}');
      const encoded = $webEncode(payload);
      $assert(!encoded.includes("<") && !encoded.includes(">") && !encoded.includes("&"));
      const decoded = $webDecode(encoded);
      $assert(Object.getPrototypeOf(decoded) === Object.prototype && !({}).polluted);
      $assert(Object.hasOwn(decoded, "__proto__") && decoded.__proto__.polluted);
      $assert(decoded.text === "</script>&" && decoded.constructor === "safe");
      $assert($webEncode({b: 2, a: 1}) === $webEncode({a: 1, b: 2}));
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn transport_rejects_container_accessors_extensions_and_sparse_arrays_without_executing_them()
{
    run(r#"
      let executed = 0;
      const getter = () => { executed++; return 1; };
      const accessorArray = [0]; Object.defineProperty(accessorArray, '0', {get: getter});
      const accessorOption = $optionBox(null); Object.defineProperty(accessorOption, '_0', {get: getter});
      const hidden = {}; Object.defineProperty(hidden, 'secret', {get: getter});
      const map = new Map(); map.entries = getter;
      const set = new Set(); set[Symbol.iterator] = getter;
      const date = new Date(); date.toISOString = getter;
      const extraArray = [1]; extraArray.extra = 2;
      const symbolArray = [1]; symbolArray[Symbol('extra')] = 2;
      class CustomMap extends Map {}
      for (const value of [accessorArray, accessorOption, hidden, map, set, date, extraArray, symbolArray, new Array(1), new CustomMap()]) {
        let error; try { $webEncode(value); } catch (caught) { error = caught; }
        $assert(error instanceof TypeError);
      }
      $assert(executed === 0);
      const decoded = $webDecode($webEncode([undefined, null]));
      $assert(decoded.length === 2 && decoded[0] === undefined && decoded[1] === null);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn transport_depth_budget_accepts_128_and_rejects_129_on_both_sides() {
    run(r#"
      const nested = depth => { let value = null; while (depth--) value = [value]; return value; };
      const wire = depth => JSON.stringify({version:1, root:['ref',0], nodes:Array.from({length:depth}, (_, index) => ['array', [index + 1 < depth ? ['ref',index + 1] : null]])});
      $webDecode($webEncode(nested(128)));
      $webDecode(wire(128));
      for (const call of [() => $webEncode(nested(129)), () => $webDecode(wire(129))]) {
        let error; try { call(); } catch (caught) { error = caught; }
        $assert(error instanceof RangeError && error.message.includes('traversal budget'));
      }
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn transport_node_budget_accepts_100000_and_rejects_100001() {
    run(r#"
      const wire = count => JSON.stringify({version:1, root:null, nodes:Array.from({length:count}, () => ['record',[]])});
      $webDecode(wire(100000));
      $webEncode(Array.from({length:99999}, () => ({})));
      for (const call of [() => $webEncode(Array.from({length:100000}, () => ({}))), () => $webDecode(wire(100001))]) {
        let error; try { call(); } catch (caught) { error = caught; }
        $assert(error instanceof RangeError && error.message.includes('node budget'));
      }
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn transport_traversal_budget_includes_unreachable_node_validation() {
    run(r#"
      const wire = count => JSON.stringify({version:1, root:['ref',0], nodes:[['array', Array(count).fill(null)]]});
      $assert($webDecode(wire(999998)).length === 999998);
      $webEncode(Array(999998).fill(null));
      for (const call of [() => $webEncode(Array(999999).fill(null)), () => $webDecode(wire(999999))]) {
        let error; try { call(); } catch (caught) { error = caught; }
        $assert(error instanceof RangeError && error.message.includes('traversal budget'));
      }
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn transport_validates_all_token_shapes_and_unreachable_graph_entries() {
    run(r#"
      const documents = [
        {version:1, nodes:[]},
        ...[['undefined',0], ['bigint','1.2'], ['bigint',''], ['number','0',false], ['number','NaN',true], ['ref',-1], ['ref',0.5], ['ref',0,'extra']].map(root => ({version:1,root,nodes:[]})),
        ...[['date','not-a-date'], ['date',42], ['array',{}], ['set',[['unknown']]], ['map',[[null]]], ['option',['ref',99]], ['record',[['x',['unknown']]]]].map(node => ({version:1,root:null,nodes:[node]})),
      ];
      for (const document of documents) {
        let error; try { $webDecode(JSON.stringify(document)); } catch (caught) { error = caught; }
        $assert(error instanceof TypeError);
      }
      const text = '</script>\u2028\u2029&<>"';
      const encoded = $webEncode({text});
      $assert(!/[<>&\u2028\u2029]/.test(encoded));
      $assert($webDecode(encoded).text === text);
      const cyclic = $webDecode(JSON.stringify({version:1,root:['ref',0],nodes:[['option',['ref',0]]]}));
      $assert($optionUnbox(cyclic) === cyclic);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn transport_rejects_invalid_wire_values_and_non_data_objects() {
    run(r#"
      const throws = fn => { let threw = false; try { fn(); } catch { threw = true; } $assert(threw); };
      for (const value of [() => {}, Symbol(), new Error("secret"), {get secret() {throw Error("must not run");}}]) {
        throws(() => $webEncode(value));
      }
      for (const payload of [
        {}, {version: 2, nodes: [], root: null}, {version: 1, nodes: [], root: ["ref", 0]},
        {version: 1, nodes: [["record", [["x", 1], ["x", 2]]]], root: ["ref", 0]},
        {version: 1, nodes: [["function", "bad"]], root: null},
        {version: 1, nodes: [["record", [[4, 2]]]], root: null},
        {version: 1, nodes: [], root: ["number", "123", false]},
      ]) throws(() => $webDecode(JSON.stringify(payload)));
    "#).await;
}
