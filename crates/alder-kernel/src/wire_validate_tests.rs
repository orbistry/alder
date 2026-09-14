use super::KERNEL_JS;

async fn run(source: &str) {
    let code = format!("{KERNEL_JS}\n{source}");
    assert_eq!(alder_runtime::execute(code, vec![]).await.unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn remote_validator_checks_exact_arity_records_and_option_fields() {
    run(r#"
      const schema = {root:0,nodes:[["tuple",[1]],["record",[["id",2],["label",3]]],["string"],["option",2]]};
      const value = [{id:"one",label:null}];
      $assert($webValidate(value,schema) === value);
      $webValidate([{id:"one",label:"name"}],schema);
      const fails = value => { let caught; try {$webValidate(value,schema);}catch(error){caught=error;} $assert(caught instanceof TypeError); };
      for (const value of [[],[{id:"one",label:null},{}],[{id:42,label:null}],[{id:"one"}],[{id:"one",label:undefined}],[{id:"one",label:null,extra:1}],{0:{id:"one",label:null},length:1}]) fails(value);
      let reads = 0;
      fails([{get id(){reads++;return "one";},label:null}]);
      $assert(reads === 0);
      const inherited = Object.create({id:"one"}); inherited.label=null; fails([inherited]);
      const symbol = {id:"one",label:null}; symbol[Symbol()]=1; fails([symbol]);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn remote_validator_distinguishes_nested_options_from_same_named_enums() {
    run(r#"
      const option = {root:0,nodes:[["option",1],["option",2],["number"]]};
      for (const value of [null,1,$optionSome(null)]) $webValidate(value,option);
      const bad = (value,schema) => {let error;try{$webValidate(value,schema);}catch(e){error=e;}$assert(error instanceof TypeError);};
      bad({$:"Some",_0:null},option);
      bad($optionBox(42),option);
      const enumeration = {root:0,nodes:[["enum",[["Some",[["_0",1]]]]],["option",2],["string"]]};
      $webValidate({$:"Some",_0:null},enumeration);
      bad($optionSome(null),enumeration);
      bad({$:"Some",_0:null,extra:1},enumeration);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn remote_validator_checks_maps_sets_dates_bigints_and_unit() {
    run(r#"
      const schema = {root:0,nodes:[["tuple",[1,2,3,4,5]],["map",6,7],["set",7],["date"],["bigint"],["unit"],["string"],["number"]]};
      const date = new Date("2026-01-01T00:00:00Z");
      $webValidate([new Map([["x",NaN]]),new Set([Infinity]),date,42n,undefined],schema);
      const fails = value => {let error;try{$webValidate(value,schema);}catch(e){error=e;}$assert(error instanceof TypeError);};
      fails([new Map([[42,1]]),new Set([1]),date,42n,undefined]);
      fails([new Map(),new Set(["wrong"]),date,42n,undefined]);
      fails([new Map(),new Set(),new Date(NaN),42n,undefined]);
      fails([new Map(),new Set(),date,42,undefined]);
      fails([new Map(),new Set(),date,42n,null]);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn remote_validator_preserves_cycles_and_bounds_depth_and_visits() {
    run(r#"
      const schema = {root:0,nodes:[["record",[["next",1],["value",2]]],["option",0],["number"]]};
      const cycle = {next:null,value:1}; cycle.next=cycle;
      $assert($webValidate(cycle,schema) === cycle);
      let list = null;
      for(let index=0;index<140;index++) list={next:list,value:index};
      let error;
      try{$webValidate(list,schema);}catch(e){error=e;}
      $assert(error instanceof TypeError);
      const wide = {root:0,nodes:[["array",1],["number"]]};
      error=null;try{$webValidate(Array(100).fill(1),wide,{maxVisits:20});}catch(e){error=e;}$assert(error instanceof TypeError);
      const wrong = {next:null,value:cycle};wrong.next=wrong;
      error=null;try{$webValidate(wrong,schema);}catch(e){error=e;}$assert(error instanceof TypeError);
      const sparse=[];sparse.length=2;
      error=null;try{$webValidate(sparse,wide);}catch(e){error=e;}$assert(error instanceof TypeError);
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn remote_validator_checks_closed_and_open_error_row_payloads() {
    run(r#"
      const open = {root:0,nodes:[["error",[[":missing",[["_0",1]]]],true],["string"]]};
      $webValidate({$:':missing',_0:'id'},open);
      const value={$:':other',_0:{nested:new Map([[42n,new Set([null])]])}};
      $webValidate(value,open);
      const fails = (value,schema=open) => {let error;try{$webValidate(value,schema);}catch(e){error=e;}$assert(error instanceof TypeError);};
      fails({$:':missing',_0:42});
      fails({$:':other',_1:'gap'});
      fails({$:':other',_0:()=>42});
      fails({$:'NotAnError'});
      fails({$:':other'},{root:0,nodes:[["error",[],false]]});
    "#).await;
}
