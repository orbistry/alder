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
async fn forms_server_reads_are_lazy_preserve_duplicates_and_reject_files() {
    run(r#"
        const request=new Request("https://example.test/signup",{method:"POST",headers:{"content-type":"application/x-www-form-urlencoded"},body:"name=Ada&tag=rust&tag=js&empty="});
        const task=$httpReadForm(request);
        $assert(!request.bodyUsed);
        const result=await $runTask(task);
        $assert(result.$==="Ok");
        $assert(JSON.stringify([...result._0])==='[["name",["Ada"]],["tag",["rust","js"]],["empty",[""]]]');
        const repeated=await $runTask(task);
        $assert(repeated.$==="Err" && repeated._0.$===":body_error");
        const data=new FormData();data.append("file",new Blob(["hello"]),"hello.txt");
        const file=await $runTask($httpReadForm(new Request("https://example.test",{method:"POST",body:data})));
        $assert(file.$==="Err" && file._0.$===":file_field" && file._0._0==="file");
    "#).await;
}

#[tokio::test(flavor = "current_thread")]
async fn forms_browser_uses_current_form_and_submitter_and_prevents_default_synchronously() {
    run(r#"
        const NativeFormData=globalThis.FormData;
        let prevented=false;
        const form={tagName:"FORM"};const submitter={name:"intent",value:"save"};
        const event={currentTarget:form,target:{tagName:"INPUT"},submitter,preventDefault(){prevented=true;}};
        globalThis.FormData=class{constructor(actual,button){$assert(actual===form && button===submitter);}entries(){return [["name","Ada"],["intent","save"],["tags","a"],["tags","b"]][Symbol.iterator]();}};
        try {
            $webPreventDefault(event);$assert(prevented);
            const result=$webFormValues(event);
            $assert(result.$==="Ok" && JSON.stringify(result._0.get("tags"))==='["a","b"]');
            $assert(result._0.get("intent")[0]==="save");
            prevented=false;let handled=false;
            const handler=$webSubmit(values=>$task(function*(){$assert(values._0.get("name")[0]==="Ada");handled=true;}));
            const task=handler(event);
            $assert(prevented && !handled);
            event.currentTarget=null;
            await $runTask(task);
            $assert(handled);
            event.currentTarget=form;
            $assert($webFormValues({currentTarget:{tagName:"DIV"}})._0.$===":invalid_form");
            globalThis.FormData=class{constructor(){throw new TypeError("invalid form");}};
            $assert($webFormValues(event)._0.$===":invalid_form");
        } finally {globalThis.FormData=NativeFormData;}
    "#).await;
}
