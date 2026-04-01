// Test if console is available in QuickJS runtime
function onSearch(query) {
    console.log("Test console log");
    var results = [{
        title: "Test",
        subtitle: "This is a test",
        action: "test-action"
    }];
    
    if (typeof globalThis.raycast !== "undefined") {
        globalThis.raycast.updateList(results);
    }
}

globalThis.onSearch = onSearch;
