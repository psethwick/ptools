// @raycast/api shim — injected into QuickJS before Raycast-style extensions run.
//
// Provides:
//  - A minimal React-like API (createElement / jsx, useState, useEffect, useRef, …)
//  - Raycast components (List, List.Item, List.Section, ActionPanel, Action, …)
//  - A reconciler that walks the rendered VNode tree and calls raycast.updateList()
//  - globalThis.__raycastBootstrap(component) — called after loading a default export
//  - globalThis.require(name)                — CJS compatibility shim
//  - globalThis.babelHelpers                 — interop helpers for oxc CJS output
//
// All internals are wrapped in an IIFE to avoid polluting globals unnecessarily.

(function () {
  "use strict";

  // ── VNode factory ────────────────────────────────────────────────────────────
  // All component constructors return a plain object tagged with $$vnode: true.

  function h(type, props) {
    var p = props || {};
    var children = [];
    for (var i = 2; i < arguments.length; i++) {
      var c = arguments[i];
      if (c == null || c === false || c === true) continue;
      if (Array.isArray(c)) {
        for (var j = 0; j < c.length; j++) {
          if (c[j] != null && c[j] !== false && c[j] !== true) {
            children.push(c[j]);
          }
        }
      } else {
        children.push(c);
      }
    }
    // Merge children from props.children too
    if (p.children != null) {
      var pc = p.children;
      if (Array.isArray(pc)) {
        for (var k = 0; k < pc.length; k++) {
          if (pc[k] != null && pc[k] !== false && pc[k] !== true) {
            children.push(pc[k]);
          }
        }
      } else if (pc !== false && pc !== true) {
        children.push(pc);
      }
    }
    // Build a clean props without children key (children are now in the array)
    var cleanProps = {};
    for (var key in p) {
      if (key !== "children" && Object.prototype.hasOwnProperty.call(p, key)) {
        cleanProps[key] = p[key];
      }
    }
    return { $$vnode: true, type: type, props: cleanProps, children: children };
  }

  // ── Per-render state ─────────────────────────────────────────────────────────

  var _states = [];
  var _stateIdx = 0;
  var _effects = [];
  var _effectIdx = 0;
  var _isRendering = false;

  // ── Navigation stack ─────────────────────────────────────────────────────────
  // Each entry saves the root component and hook state so we can restore it on pop.

  var _navStack = [];

  function _saveNavState() {
    return {
      component: _rootComponent,
      states: _states.slice(),
      effects: _effects.slice(),
      searchCb: _List ? _List.__searchCb : null,
      gridSearchCb: _Grid ? _Grid.__searchCb : null,
    };
  }

  function _restoreNavState(saved) {
    _rootComponent = saved.component;
    _states = saved.states;
    _effects = saved.effects;
    if (_List) _List.__searchCb = saved.searchCb;
    if (_Grid) _Grid.__searchCb = saved.gridSearchCb;
  }

  function _navigationPush(element) {
    _navStack.push(_saveNavState());
    _states = [];
    _effects = [];
    if (_List) _List.__searchCb = null;
    if (_Grid) _Grid.__searchCb = null;
    if (element && element.$$vnode) {
      var type = element.type;
      var props = element.props || {};
      var ch = element.children || [];
      _rootComponent = function () {
        if (typeof type === "function") {
          var callProps =
            ch.length > 0
              ? Object.assign({}, props, {
                  children: ch.length === 1 ? ch[0] : ch,
                })
              : props;
          return type(callProps);
        }
        return element;
      };
    } else if (typeof element === "function") {
      _rootComponent = element;
    } else {
      _rootComponent = function () { return element; };
    }
    // Extract navigation title for the breadcrumb bar.
    var navTitle = "View";
    if (element && element.props && typeof element.props.navigationTitle === "string") {
      navTitle = element.props.navigationTitle;
    } else if (element && element.type && typeof element.type.navigationTitle === "string") {
      navTitle = element.type.navigationTitle;
    }
    // Notify Rust so it can track nav depth and show the breadcrumb bar.
    if (typeof globalThis.raycast !== "undefined" && typeof globalThis.raycast.navigate === "function") {
      globalThis.raycast.navigate("push-view:" + navTitle);
    }
    _doRender();
  }

  function _navigationPop() {
    if (_navStack.length === 0) return;
    _restoreNavState(_navStack.pop());
    // Notify Rust so it can decrement nav depth and hide the breadcrumb entry.
    if (typeof globalThis.raycast !== "undefined" && typeof globalThis.raycast.navigate === "function") {
      globalThis.raycast.navigate("pop-view");
    }
    _doRender();
  }

  // ── Per-render action handler maps ───────────────────────────────────────────
  // Populated by _extractAction during each render; read by onAction.

  var _actionHandlers = {};
  var _pushTargets = {};
  var _pushCounter = 0;

  function useState(initial) {
    var idx = _stateIdx++;
    if (_states[idx] === undefined) {
      _states[idx] = typeof initial === "function" ? initial() : initial;
    }
    var capturedIdx = idx;
    function setState(val) {
      _states[capturedIdx] =
        typeof val === "function" ? val(_states[capturedIdx]) : val;
      if (!_isRendering) {
        _scheduleRender();
      }
    }
    return [_states[capturedIdx], setState];
  }

  function useEffect(fn, deps) {
    var idx = _effectIdx++;
    var prev = _effects[idx];
    var run = true;
    if (prev && deps) {
      run = false;
      for (var i = 0; i < deps.length; i++) {
        if (deps[i] !== prev.deps[i]) {
          run = true;
          break;
        }
      }
    }
    if (run) {
      _effects[idx] = { deps: deps ? deps.slice() : null };
      // Run effect as a microtask so it doesn't block the current render
      Promise.resolve().then(function () {
        try {
          fn();
        } catch (e) {
          console.log("[raycast-shim] useEffect error: " + String(e));
        }
      });
    }
  }

  function useRef(initial) {
    var idx = _stateIdx++;
    if (_states[idx] === undefined) {
      _states[idx] = { current: initial };
    }
    return _states[idx];
  }

  function useCallback(fn) {
    return fn;
  }

  function useMemo(fn) {
    return fn();
  }

  function useNavigation() {
    return { push: _navigationPush, pop: _navigationPop };
  }

  function useFetch(url, options) {
    var _url = typeof url === "function" ? url() : url;
    var _opts = options || {};
    var mapFn = _opts.mapResult || function (x) { return x; };
    var initData = _opts.initialData !== undefined ? _opts.initialData : undefined;

    var stateRef = useState({ data: initData, isLoading: true, error: undefined });
    var data = stateRef[0].data;
    var isLoading = stateRef[0].isLoading;
    var error = stateRef[0].error;
    var setState = stateRef[1];

    useEffect(function () {
      if (!_url) return;
      setState(function (s) { return { data: s.data, isLoading: true, error: undefined }; });
      fetch(_url, { method: _opts.method || "GET", headers: _opts.headers })
        .then(function (res) {
          if (!res.ok) throw new Error("HTTP " + res.status);
          return res.json();
        })
        .then(function (json) {
          setState({ data: mapFn(json), isLoading: false, error: undefined });
        })
        .catch(function (err) {
          setState({ data: initData, isLoading: false, error: err });
        });
    }, [_url]);

    return { data: data, isLoading: isLoading, error: error, revalidate: function () {} };
  }

  function usePromise(fn, args, options) {
    var _args = args || [];
    var _opts = options || {};
    var initData = _opts.initialData !== undefined ? _opts.initialData : undefined;

    var stateRef = useState({ data: initData, isLoading: true, error: undefined });
    var data = stateRef[0].data;
    var isLoading = stateRef[0].isLoading;
    var error = stateRef[0].error;
    var setState = stateRef[1];

    useEffect(function () {
      setState(function (s) { return { data: s.data, isLoading: true, error: undefined }; });
      fn.apply(null, _args)
        .then(function (result) {
          setState({ data: result, isLoading: false, error: undefined });
        })
        .catch(function (err) {
          setState({ data: initData, isLoading: false, error: err });
        });
    }, _args);

    return { data: data, isLoading: isLoading, error: error, revalidate: function () {} };
  }

  // ── Component type identities ────────────────────────────────────────────────
  // These functions act as type tags for the reconciler.

  function _List(props) {
    if (props && typeof props.onSearchTextChange === "function") {
      _List.__searchCb = props.onSearchTextChange;
    }
    return h("__list__", props);
  }

  function _ListItem(props) {
    return h(_ListItem, props);
  }

  function _ListSection(props) {
    return h("__section__", props);
  }

  function _ListEmptyView(props) {
    return h("__emptyview__", props);
  }

  function _ActionPanel(props) {
    return h(_ActionPanel, props);
  }

  function _Action(props) {
    return h(_Action, props);
  }

  function _ActionCopyToClipboard(props) {
    return h(_ActionCopyToClipboard, props);
  }

  function _ActionOpenInBrowser(props) {
    return h(_ActionOpenInBrowser, props);
  }

  function _ActionPush(props) {
    return h(_ActionPush, props);
  }

  function _ActionShowInFinder(props) {
    return h(_ActionShowInFinder, props);
  }

  function _ActionTrash(props) {
    return h(_ActionTrash, props);
  }

  function _Grid(props) {
    if (props && typeof props.onSearchTextChange === "function") {
      _Grid.__searchCb = props.onSearchTextChange;
    }
    return h("__grid__", props);
  }

  function _GridItem(props) {
    return h(_GridItem, props);
  }

  function _GridSection(props) {
    return h("__gridsection__", props);
  }

  function _GridEmptyView(props) {
    return h("__gridemptyview__", props);
  }

  _Grid.Item = _GridItem;
  _Grid.Section = _GridSection;
  _Grid.EmptyView = _GridEmptyView;

  function _Detail(props) {
    return h("__detail__", props);
  }

  function _DetailMetadata(props) {
    return h("__detail_metadata__", props);
  }
  function _DetailMetadataLabel(props) {
    return h("__detail_metadata_label__", props);
  }
  function _DetailMetadataLink(props) {
    return h("__detail_metadata_link__", props);
  }
  function _DetailMetadataSeparator(props) {
    return h("__detail_metadata_separator__", props);
  }
  function _DetailMetadataTagList(props) {
    return h("__detail_metadata_taglist__", props);
  }
  function _DetailMetadataTagListItem(props) {
    return h("__detail_metadata_taglist_item__", props);
  }
  _DetailMetadata.Label = _DetailMetadataLabel;
  _DetailMetadata.Link = _DetailMetadataLink;
  _DetailMetadata.Separator = _DetailMetadataSeparator;
  _DetailMetadata.TagList = _DetailMetadataTagList;
  _DetailMetadataTagList.Item = _DetailMetadataTagListItem;

  function _FormTextField(props) { return h(_FormTextField, props); }
  function _FormCheckbox(props)  { return h(_FormCheckbox,  props); }
  function _FormDropdown(props)  { return h(_FormDropdown,  props); }
  function _FormDropdownItem(props) { return h(_FormDropdownItem, props); }

  _FormDropdown.Item = _FormDropdownItem;

  function _Form(props) { return h("__form__", props); }
  _Form.TextField = _FormTextField;
  _Form.Checkbox  = _FormCheckbox;
  _Form.Dropdown  = _FormDropdown;

  // ── MenuBarExtra components ───────────────────────────────────────────────────
  // MenuBarExtra is a macOS menu-bar-item component. In the launcher context we
  // degrade gracefully: items are rendered as a normal List so the extension runs.

  function _MenuBarExtra(props) {
    return h("__menubarextra__", props);
  }

  function _MenuBarExtraItem(props) {
    return h(_MenuBarExtraItem, props);
  }

  function _MenuBarExtraSection(props) {
    return h("__menubarextra_section__", props);
  }

  _MenuBarExtra.Item = _MenuBarExtraItem;
  _MenuBarExtra.Section = _MenuBarExtraSection;

  _List.Item = _ListItem;
  _List.Section = _ListSection;
  _List.EmptyView = _ListEmptyView;
  _ListItem.Detail = _Detail;
  _Detail.Metadata = _DetailMetadata;
  _Action.CopyToClipboard = _ActionCopyToClipboard;
  _Action.OpenInBrowser = _ActionOpenInBrowser;
  _Action.Push = _ActionPush;
  _Action.ShowInFinder = _ActionShowInFinder;
  _Action.Trash = _ActionTrash;
  _Action.SubmitForm = function (props) {
    return h(_Action.SubmitForm, props);
  };

  // ── Icon / Color stubs ───────────────────────────────────────────────────────

  var Icon = {
    Globe: "🌐",
    Star: "⭐",
    Link: "🔗",
    Clipboard: "📋",
    MagnifyingGlass: "🔍",
    List: "📋",
    Person: "👤",
    Document: "📄",
    Gear: "⚙️",
    ArrowRight: "→",
    ArrowLeft: "←",
    Plus: "+",
    Minus: "-",
    Trash: "🗑️",
    Checkmark: "✓",
    XMark: "✗",
    Eye: "👁",
    EyeSlash: "🙈",
    QuestionMark: "?",
    ExclamationMark: "!",
    Info: "ℹ️",
    Warning: "⚠️",
    Upload: "⬆️",
    Download: "⬇️",
    Folder: "📁",
    Image: "🖼️",
    Video: "🎬",
    Music: "🎵",
    Code: "💻",
    Terminal: "🖥️",
    BoltFilled: "⚡",
    Clock: "🕐",
    Calendar: "📅",
  };

  var Color = {
    Red: { light: "#ff0000", dark: "#ff0000" },
    Green: { light: "#00aa00", dark: "#00ff00" },
    Blue: { light: "#0000ff", dark: "#4444ff" },
    Yellow: { light: "#ccaa00", dark: "#ffff00" },
    Purple: { light: "#800080", dark: "#cc00cc" },
    Orange: { light: "#cc6600", dark: "#ff8800" },
    PrimaryText: "#ffffff",
    SecondaryText: "#999999",
  };

  // ── Action extraction (from action panel VNode) ──────────────────────────────

  /// Format a Raycast shortcut object like { modifiers: ["cmd","shift"], key: "n" }
  /// into a human-readable label like "⌘⇧N".
  function _formatShortcut(shortcut) {
    if (!shortcut || typeof shortcut !== "object") return null;
    var mods = Array.isArray(shortcut.modifiers) ? shortcut.modifiers : [];
    var key = String(shortcut.key || "");
    var label = "";
    if (mods.indexOf("cmd") !== -1)    label += "⌘";
    if (mods.indexOf("ctrl") !== -1)   label += "⌃";
    if (mods.indexOf("opt") !== -1 || mods.indexOf("alt") !== -1) label += "⌥";
    if (mods.indexOf("shift") !== -1)  label += "⇧";
    if (key) label += key.toUpperCase();
    return label || null;
  }

  /// Extract ALL actions from an ActionPanel VNode, returning an array of
  /// { title, action, icon, shortcut } objects (one per <Action> child).
  function _extractAllActions(node) {
    if (node == null) return [];
    if (Array.isArray(node)) {
      var all = [];
      for (var i = 0; i < node.length; i++) {
        var sub = _extractAllActions(node[i]);
        for (var j = 0; j < sub.length; j++) all.push(sub[j]);
      }
      return all;
    }
    if (!node.$$vnode) return [];
    var t = node.type;
    var p = node.props || {};
    var ch = node.children || [];

    if (t === _ActionCopyToClipboard) {
      var ctcIcon = p.icon ? (typeof p.icon === "string" ? p.icon : (p.icon.source || null)) : null;
      return [{ title: String(p.title || "Copy to Clipboard"), action: "clipboard-copy:" + String(p.content || ""), icon: ctcIcon, shortcut: _formatShortcut(p.shortcut) }];
    }
    if (t === _ActionShowInFinder) {
      var sifIcon = p.icon ? (typeof p.icon === "string" ? p.icon : (p.icon.source || null)) : null;
      return [{ title: String(p.title || "Show in Finder"), action: "show-in-finder:" + String(p.path || ""), icon: sifIcon, shortcut: _formatShortcut(p.shortcut) }];
    }
    if (t === _ActionTrash) {
      var trashIcon = p.icon ? (typeof p.icon === "string" ? p.icon : (p.icon.source || null)) : null;
      return [{ title: String(p.title || "Move to Trash"), action: "trash-file:" + String(p.paths && p.paths[0] || p.path || ""), icon: trashIcon, shortcut: _formatShortcut(p.shortcut) }];
    }
    if (t === _ActionOpenInBrowser) {
      var oibIcon = p.icon ? (typeof p.icon === "string" ? p.icon : (p.icon.source || null)) : null;
      return [{ title: String(p.title || "Open in Browser"), action: "open-url:" + String(p.url || ""), icon: oibIcon, shortcut: _formatShortcut(p.shortcut) }];
    }
    if (t === _ActionPush) {
      var pushStr = _extractAction(node); // reuse existing push-id generator
      if (pushStr !== "noop") {
        var apIcon = p.icon ? (typeof p.icon === "string" ? p.icon : (p.icon.source || null)) : null;
        return [{ title: String(p.title || "Push"), action: pushStr, icon: apIcon, shortcut: _formatShortcut(p.shortcut) }];
      }
      return [];
    }
    if (t === _Action) {
      var actStr = _extractAction(node); // reuse handler-registration logic
      if (actStr === "noop") return [];
      var actIcon = p.icon ? (typeof p.icon === "string" ? p.icon : (p.icon.source || null)) : null;
      return [{ title: String(p.title || ""), action: actStr, icon: actIcon, shortcut: _formatShortcut(p.shortcut) }];
    }
    if (t === _Action.SubmitForm) {
      return [];
    }
    if (t === _ActionPanel || typeof t === "string") {
      return _extractAllActions(ch);
    }
    if (typeof t === "function") {
      try { return _extractAllActions(t(p)); } catch (e) { return []; }
    }
    return _extractAllActions(ch);
  }

  function _extractAction(node) {
    if (node == null) return "noop";
    if (typeof node === "string") return "noop";

    if (Array.isArray(node)) {
      for (var i = 0; i < node.length; i++) {
        var a = _extractAction(node[i]);
        if (a !== "noop") return a;
      }
      return "noop";
    }

    if (!node.$$vnode) return "noop";
    var t = node.type;
    var p = node.props || {};
    var ch = node.children || [];

    if (t === _ActionCopyToClipboard) {
      return "clipboard-copy:" + String(p.content || "");
    }
    if (t === _ActionShowInFinder) {
      return "show-in-finder:" + String(p.path || "");
    }
    if (t === _ActionTrash) {
      return "trash-file:" + String(p.paths && p.paths[0] || p.path || "");
    }
    if (t === _ActionOpenInBrowser) {
      return "open-url:" + String(p.url || "");
    }
    if (t === _Action.SubmitForm) {
      return "noop";
    }
    if (t === _ActionPush) {
      if (p.target != null) {
        var pushId = "push_" + (_pushCounter++);
        _pushTargets[pushId] = p.target;
        var extNamePush = globalThis.__raycastExtensionName || "js-extension";
        return extNamePush + ":js-push:" + pushId;
      }
      return "noop";
    }
    if (t === _Action) {
      if (typeof p.onAction === "function") {
        var actionTitle = String(p.title || "action");
        _actionHandlers[actionTitle] = p.onAction;
        var extNameAct = globalThis.__raycastExtensionName || "js-extension";
        return extNameAct + ":js-action:" + actionTitle;
      }
      return "noop";
    }
    if (t === _ActionPanel || typeof t === "string") {
      // Recurse into children
      var children = ch;
      var r = _extractAction(children);
      return r;
    }
    if (typeof t === "function") {
      try {
        var rendered = t(p);
        return _extractAction(rendered);
      } catch (e) {
        return "noop";
      }
    }
    return _extractAction(ch);
  }

  // ── Metadata extractor ───────────────────────────────────────────────────────

  function _extractMetadata(vnode) {
    if (!vnode || !vnode.$$vnode || vnode.type !== "__detail_metadata__") return [];
    var rows = [];
    var metaChildren = vnode.children || [];
    if (!Array.isArray(metaChildren)) metaChildren = [metaChildren];
    for (var i = 0; i < metaChildren.length; i++) {
      var child = metaChildren[i];
      if (!child || !child.$$vnode) continue;
      var cp = child.props || {};
      if (child.type === "__detail_metadata_label__") {
        rows.push({ type: "label", title: String(cp.title || ""), text: cp.text != null ? String(cp.text) : null });
      } else if (child.type === "__detail_metadata_link__") {
        rows.push({ type: "link", title: String(cp.title || ""), text: String(cp.text || ""), target: String(cp.target || "") });
      } else if (child.type === "__detail_metadata_separator__") {
        rows.push({ type: "separator" });
      } else if (child.type === "__detail_metadata_taglist__") {
        var tags = [];
        var tagChildren = cp.children || [];
        if (!Array.isArray(tagChildren)) tagChildren = [tagChildren];
        for (var j = 0; j < tagChildren.length; j++) {
          var tc = tagChildren[j];
          if (!tc || !tc.$$vnode || tc.type !== "__detail_metadata_taglist_item__") continue;
          var tcp = tc.props || {};
          tags.push({ text: String(tcp.text || ""), color: tcp.color != null ? String(tcp.color) : null });
        }
        rows.push({ type: "tagList", title: String(cp.title || ""), tags: tags });
      }
    }
    return rows;
  }

  // ── Reconciler: VNode tree → ExtensionItem list ──────────────────────────────

  function _renderNode(node) {
    if (node == null || node === false || node === true) return [];
    if (typeof node === "string" || typeof node === "number") return [];

    if (Array.isArray(node)) {
      var result = [];
      for (var i = 0; i < node.length; i++) {
        var items = _renderNode(node[i]);
        for (var j = 0; j < items.length; j++) result.push(items[j]);
      }
      return result;
    }

    if (!node.$$vnode) return [];

    var type = node.type;
    var props = node.props || {};
    var children = node.children || [];

    // List.Item → ExtensionItem
    if (type === _ListItem) {
      var icon = props.icon;
      var iconStr = null;
      if (typeof icon === "string") {
        iconStr = icon;
      } else if (icon && icon.source) {
        iconStr = String(icon.source);
      } else if (icon && typeof icon.value === "string") {
        iconStr = icon.value;
      }

      var detail = null;
      if (props.detail) {
        // Could be a string or a Detail.Metadata component
        if (typeof props.detail === "string") {
          detail = props.detail;
        } else if (props.detail.$$vnode && props.detail.type === "__detail__") {
          // <List.Item.Detail markdown="..." /> or <Detail markdown="..." />
          detail = String(props.detail.props.markdown || "");
        }
      }

      var detailMetadata = [];
      if (props.detail && props.detail.$$vnode && props.detail.type === "__detail__") {
        if (props.detail.props.metadata) {
          detailMetadata = _extractMetadata(props.detail.props.metadata);
        }
      }

      var accessories = [];
      if (Array.isArray(props.accessories)) {
        for (var ai = 0; ai < props.accessories.length; ai++) {
          var acc = props.accessories[ai];
          if (!acc) continue;
          if (acc.text != null) {
            accessories.push({ text: String(acc.text) });
          } else if (acc.tag != null && acc.tag.value != null) {
            accessories.push({ tag: { value: String(acc.tag.value) } });
          }
        }
      }

      return [
        {
          title: String(props.title || ""),
          subtitle: props.subtitle != null ? String(props.subtitle) : null,
          icon: iconStr,
          action: _extractAction(props.actions),
          id: props.id != null ? String(props.id) : null,
          detail: detail,
          detailMetadata: detailMetadata,
          accessories: accessories,
          extraActions: _extractAllActions(props.actions),
        },
      ];
    }

    // Grid.Item → ExtensionItem (content maps to icon)
    if (type === _GridItem) {
      var gContent = props.content;
      var gIconStr = null;
      if (typeof gContent === "string") {
        gIconStr = gContent;
      } else if (gContent && gContent.source) {
        gIconStr = String(gContent.source);
      }
      // Fallback: icon prop
      if (!gIconStr) {
        var gIcon = props.icon;
        if (typeof gIcon === "string") gIconStr = gIcon;
        else if (gIcon && gIcon.source) gIconStr = String(gIcon.source);
      }
      return [
        {
          title: String(props.title || ""),
          subtitle: props.subtitle != null ? String(props.subtitle) : null,
          icon: gIconStr,
          action: _extractAction(props.actions),
          id: props.id != null ? String(props.id) : null,
          detail: null,
          accessories: [],
          extraActions: _extractAllActions(props.actions),
        },
      ];
    }

    // Grid container — compute column count from itemSize, stamp on non-header children
    if (type === "__grid__") {
      var gItemSize = props && props.itemSize;
      var gCols = gItemSize === "small" ? 5 : gItemSize === "large" ? 3 : 4;
      var gAllItems = _renderNode(children);
      for (var gi = 0; gi < gAllItems.length; gi++) {
        if (gAllItems[gi].action !== "::section::") {
          gAllItems[gi].gridColumns = gCols;
        }
      }
      return gAllItems;
    }

    // Grid.Section — emit a header sentinel then recurse into children
    if (type === "__gridsection__") {
      var gSectionTitle = String(props.title || "");
      var gHeaderItem = {
        title: gSectionTitle,
        subtitle: null,
        icon: null,
        action: "::section::",
        id: "::section::" + gSectionTitle,
        detail: null,
      };
      var gChildItems = _renderNode(children);
      return [gHeaderItem].concat(gChildItems);
    }

    // Grid.EmptyView — emit a sentinel item with id "::empty::"
    if (type === "__gridemptyview__") {
      var gevIcon = props.icon;
      var gevIconStr = null;
      if (typeof gevIcon === "string") gevIconStr = gevIcon;
      else if (gevIcon && gevIcon.source) gevIconStr = String(gevIcon.source);
      return [
        {
          title: String(props.title || "No Results"),
          subtitle: props.description != null ? String(props.description) : null,
          icon: gevIconStr,
          action: "::empty::",
          id: "::empty::",
          detail: null,
        },
      ];
    }

    // List.EmptyView — emit a sentinel item with id "::empty::"
    if (type === "__emptyview__") {
      var evIcon = props.icon;
      var evIconStr = null;
      if (typeof evIcon === "string") {
        evIconStr = evIcon;
      } else if (evIcon && evIcon.source) {
        evIconStr = String(evIcon.source);
      }
      return [
        {
          title: String(props.title || "No Results"),
          subtitle: props.description != null ? String(props.description) : null,
          icon: evIconStr,
          action: "::empty::",
          id: "::empty::",
          detail: null,
        },
      ];
    }

    // List.Section — emit a header sentinel then recurse into children
    if (type === "__section__") {
      var sectionTitle = String(props.title || "");
      var headerItem = {
        title: sectionTitle,
        subtitle: null,
        icon: null,
        action: "::section::",
        id: "::section::" + sectionTitle,
        detail: null,
      };
      var childItems = _renderNode(children);
      return [headerItem].concat(childItems);
    }

    // MenuBarExtra.Item → ExtensionItem (degraded: rendered as a list item)
    if (type === _MenuBarExtraItem) {
      var mbeIcon = props.icon;
      var mbeIconStr = null;
      if (typeof mbeIcon === "string") {
        mbeIconStr = mbeIcon;
      } else if (mbeIcon && mbeIcon.source) {
        mbeIconStr = String(mbeIcon.source);
      }

      var mbeAction = "noop";
      if (typeof props.onAction === "function") {
        var mbeTitle = String(props.title || "action");
        _actionHandlers[mbeTitle] = props.onAction;
        var extNameMbe = globalThis.__raycastExtensionName || "js-extension";
        mbeAction = extNameMbe + ":js-action:" + mbeTitle;
      }

      return [
        {
          title: String(props.title || ""),
          subtitle: props.subtitle != null ? String(props.subtitle) : null,
          icon: mbeIconStr,
          action: mbeAction,
          id: props.id != null ? String(props.id) : null,
          detail: null,
          accessories: [],
          extraActions: [],
        },
      ];
    }

    // MenuBarExtra.Section → section header sentinel then recurse into children
    if (type === "__menubarextra_section__") {
      var mbesSectionTitle = String(props.title || "");
      var mbesHeaderItem = {
        title: mbesSectionTitle,
        subtitle: null,
        icon: null,
        action: "::section::",
        id: "::section::" + mbesSectionTitle,
        detail: null,
      };
      var mbesChildItems = _renderNode(children);
      return [mbesHeaderItem].concat(mbesChildItems);
    }

    // Functional component: call and recurse
    if (typeof type === "function") {
      try {
        // Pass children through props so container components (List, ActionPanel, etc.)
        // receive their children and can forward them in their own VNode output.
        var callProps =
          children.length > 0
            ? Object.assign({}, props, {
                children: children.length === 1 ? children[0] : children,
              })
            : props;
        var rendered = type(callProps);
        // Guard: if calling the component returns a VNode of the same type (self-referential
        // pattern used by _ActionPanel, _Action, _FormTextField, etc.), do not recurse further.
        // These components are never direct list items; returning [] is correct here.
        if (rendered && rendered.$$vnode && rendered.type === type) {
          return [];
        }
        return _renderNode(rendered);
      } catch (e) {
        console.log("[raycast-shim] component render error: " + String(e));
        return [];
      }
    }

    // Fragment, List, Section, etc. — recurse into children
    return _renderNode(children);
  }

  // ── Form helpers ──────────────────────────────────────────────────────────────

  var _formSubmitHandler = null;

  function _walkForSubmitForm(node) {
    if (!node || !node.$$vnode) return;
    if (node.type === _Action.SubmitForm) {
      _formSubmitHandler = node.props && node.props.onSubmit;
      return;
    }
    // Recurse into the VNode's children array only.
    // Do NOT call node.type() — self-referential components like _ActionPanel
    // return h(_ActionPanel, ...) causing unbounded recursion.
    // Children are already collected by h() including props.children, so this is sufficient.
    var ch = node.children || [];
    for (var i = 0; i < ch.length; i++) {
      _walkForSubmitForm(ch[i]);
    }
  }

  function _extractFormDef(formNode) {
    var fields = [];
    _formSubmitHandler = null;

    var children = formNode.children || [];
    for (var i = 0; i < children.length; i++) {
      var child = children[i];
      if (!child || !child.$$vnode) continue;
      var p = child.props || {};

      if (child.type === _FormTextField) {
        fields.push({
          type: "textfield",
          id: String(p.id || ""),
          title: String(p.title || ""),
          placeholder: p.placeholder ? String(p.placeholder) : null,
          defaultValue: p.defaultValue != null ? String(p.defaultValue) : "",
        });
      } else if (child.type === _FormCheckbox) {
        fields.push({
          type: "checkbox",
          id: String(p.id || ""),
          title: String(p.title || ""),
          label: p.label ? String(p.label) : "",
          defaultValue: p.defaultValue === true || p.defaultValue === "true",
        });
      } else if (child.type === _FormDropdown) {
        var options = [];
        var dropChildren = child.children || [];
        for (var j = 0; j < dropChildren.length; j++) {
          var dc = dropChildren[j];
          if (dc && dc.$$vnode && dc.type === _FormDropdownItem) {
            var dp = dc.props || {};
            options.push({
              value: String(dp.value || ""),
              title: String(dp.title || dp.value || ""),
            });
          }
        }
        fields.push({
          type: "dropdown",
          id: String(p.id || ""),
          title: String(p.title || ""),
          options: options,
          defaultValue: p.defaultValue != null
            ? String(p.defaultValue)
            : (options.length > 0 ? options[0].value : ""),
        });
      }
    }

    // Walk actions prop to find Action.SubmitForm handler
    var actionsNode = formNode.props && formNode.props.actions;
    if (actionsNode) {
      _walkForSubmitForm(actionsNode);
    }

    return { fields: fields };
  }

  // ── Render scheduler ─────────────────────────────────────────────────────────

  var _rootComponent = null;
  var _searchText = "";
  var _pendingRender = false;

  function _doRender() {
    _pendingRender = false;
    if (!_rootComponent) return;

    _isRendering = true;
    _stateIdx = 0;
    _effectIdx = 0;
    try {
      var tree = _rootComponent({ searchText: _searchText });
      // Detect a form at the root. JSX h(_Form, ...) produces {type: _Form},
      // while manually constructed vnodes use {type: "__form__"}.
      // We test for both so we never fall through to _renderNode with form
      // field components that would cause self-referential infinite recursion.
      var isForm = tree && tree.$$vnode &&
        (tree.type === "__form__" || tree.type === _Form);
      if (isForm) {
        var formDef = _extractFormDef(tree);
        raycast.updateList([{
          id: "::form::",
          title: "Form",
          action: "noop",
          detail: JSON.stringify(formDef),
        }]);
      } else {
        var items = _renderNode(tree);
        raycast.updateList(items);
      }
    } catch (e) {
      console.log("[raycast-shim] top-level render error: " + String(e));
    }
    _isRendering = false;
  }

  function _scheduleRender() {
    if (!_pendingRender) {
      _pendingRender = true;
      Promise.resolve().then(_doRender);
    }
  }

  // ── showToast ────────────────────────────────────────────────────────────────

  function showToast(opts) {
    var style = (opts && opts.style) ? String(opts.style) : "success";
    var title = (opts && opts.title) ? String(opts.title) : "";
    var msg   = (opts && opts.message) ? String(opts.message) : "";
    if (globalThis.raycast && typeof globalThis.raycast.showToast === "function") {
      globalThis.raycast.showToast(style, title, msg);
    } else {
      console.log("[Toast] " + title + (msg ? ": " + msg : ""));
    }
  }

  // ── Bootstrap ────────────────────────────────────────────────────────────────
  // Called by the extension load path after extracting the default export.

  function __raycastBootstrap(component) {
    _rootComponent = component;

    globalThis.onSearch = function (query) {
      _searchText = query;
      // Notify any onSearchTextChange callback registered by List or Grid
      if (typeof _List.__searchCb === "function") {
        _List.__searchCb(query);
      }
      if (typeof _Grid.__searchCb === "function") {
        _Grid.__searchCb(query);
      }
      _doRender();
    };

    globalThis.onAction = function (action, id) {
      if (action === "pop-view") {
        // Escape pressed from Rust while nav_depth > 0 — pop the JS navigation stack.
        // _navigationPop() will call raycast.navigate("pop-view") to decrement nav_depth in Rust.
        _navigationPop();
      } else if (action.startsWith("form-submit::")) {
        var json = action.slice("form-submit::".length);
        try {
          var values = JSON.parse(json);
          if (typeof _formSubmitHandler === "function") {
            _formSubmitHandler(values);
          }
        } catch (e) {
          console.log("[form] parse error: " + e);
        }
      } else if (action.startsWith("js-action:")) {
        var actionTitle = action.slice("js-action:".length);
        var handler = _actionHandlers[actionTitle];
        if (typeof handler === "function") {
          try {
            handler();
          } catch (e) {
            console.log("[action] handler error: " + String(e));
          }
        }
      } else if (action.startsWith("js-push:")) {
        var pushId = action.slice("js-push:".length);
        var target = _pushTargets[pushId];
        if (target != null) {
          _navigationPush(target);
        }
      }
    };

    // Initial render (empty search)
    _doRender();
  }

  // ── Minimal React object ─────────────────────────────────────────────────────

  var React = {
    createElement: h,
    Fragment: "__fragment__",
    useState: useState,
    useEffect: useEffect,
    useRef: useRef,
    useCallback: useCallback,
    useMemo: useMemo,
    useContext: function () {
      return undefined;
    },
    createContext: function (defaultValue) {
      return { _current: defaultValue };
    },
    memo: function (component) {
      return component;
    },
    forwardRef: function (component) {
      return component;
    },
    Children: {
      map: function (children, fn) {
        if (!Array.isArray(children)) children = children ? [children] : [];
        return children.map(fn);
      },
      toArray: function (children) {
        if (!Array.isArray(children)) return children ? [children] : [];
        return children;
      },
    },
  };

  // ── JSX runtime (for automatic JSX transform) ────────────────────────────────

  var jsxRuntime = {
    jsx: h,
    jsxs: h,
    Fragment: "__fragment__",
  };

  // ── Clipboard ────────────────────────────────────────────────────────────────

  var Clipboard = {
    readText: function () {
      var text = (globalThis.raycast && typeof globalThis.raycast.clipboardRead === "function")
        ? globalThis.raycast.clipboardRead()
        : "";
      return Promise.resolve(text);
    },
    copy: function (content) {
      var text = (content && typeof content === "object" && content.text != null)
        ? String(content.text)
        : String(content != null ? content : "");
      if (globalThis.raycast && typeof globalThis.raycast.clipboardWrite === "function") {
        globalThis.raycast.clipboardWrite(text);
      }
      return Promise.resolve();
    },
    paste: function (content) {
      // paste into focused app is complex; fall back to copy for now
      return Clipboard.copy(content);
    },
  };

  // ── Module object (returned by require('@raycast/api')) ──────────────────────

  var raycastApiModule = {
    // Components
    List: _List,
    ActionPanel: _ActionPanel,
    Action: _Action,
    Detail: _Detail,
    Form: _Form,
    Grid: _Grid,
    MenuBarExtra: _MenuBarExtra,
    // Hooks — exported so `import { useState } from "@raycast/api"` works
    useState: useState,
    useEffect: useEffect,
    useRef: useRef,
    useCallback: useCallback,
    useMemo: useMemo,
    useNavigation: useNavigation,
    useFetch: useFetch,
    usePromise: usePromise,
    // Utilities
    showToast: showToast,
    closeMainWindow: function () {
      if (globalThis.raycast && typeof globalThis.raycast.hideWindow === "function") {
        globalThis.raycast.hideWindow();
      }
    },
    open: function (url) {
      if (globalThis.raycast && typeof globalThis.raycast.open === "function") {
        globalThis.raycast.open(String(url));
      } else {
        console.log("[open] " + String(url));
      }
    },
    getPreferenceValues: function () {
      if (globalThis.raycast && typeof globalThis.raycast.getPreferences === "function") {
        try {
          return JSON.parse(globalThis.raycast.getPreferences());
        } catch (_) {}
      }
      return {};
    },
    getSelectedText: function () {
      if (
        typeof globalThis.raycast !== "undefined" &&
        typeof globalThis.raycast.getSelectedText === "function"
      ) {
        return Promise.resolve(globalThis.raycast.getSelectedText());
      }
      return Promise.resolve("");
    },
    // Constants
    Icon: Icon,
    Color: Color,
    Keyboard: {
      Key: {
        Return: "return",
        Escape: "escape",
        Delete: "delete",
        Tab: "tab",
      },
      Modifier: {
        Cmd: "cmd",
        Opt: "opt",
        Ctrl: "ctrl",
        Shift: "shift",
      },
    },
    environment: {
      isDevelopment:
        globalThis.raycast && typeof globalThis.raycast.isDevelopment === "function"
          ? globalThis.raycast.isDevelopment()
          : true,
      extensionName:
        globalThis.raycast && typeof globalThis.raycast.extensionName === "function"
          ? globalThis.raycast.extensionName()
          : "",
      commandName:
        globalThis.raycast && typeof globalThis.raycast.commandName === "function"
          ? globalThis.raycast.commandName()
          : "",
      theme:
        globalThis.raycast && typeof globalThis.raycast.getTheme === "function"
          ? globalThis.raycast.getTheme()
          : "dark",
      textSize: "medium",
      launchType: "userInitiated",
      supportPath:
        globalThis.raycast && typeof globalThis.raycast.supportPath === "function"
          ? globalThis.raycast.supportPath()
          : "",
      assetsPath:
        globalThis.raycast && typeof globalThis.raycast.assetsPath === "function"
          ? globalThis.raycast.assetsPath()
          : "",
    },
    LaunchType: { UserInitiated: "userInitiated", Background: "background" },
    Toast: {
      Style: { Success: "success", Failure: "failure", Animated: "animated" },
    },
    // React compat (some extensions import React from @raycast/api)
    React: React,
    Clipboard: Clipboard,
    LocalStorage: {
      getItem: function (key) {
        var val =
          globalThis.raycast &&
          typeof globalThis.raycast.storageGet === "function"
            ? globalThis.raycast.storageGet(key)
            : null;
        if (val === null || val === undefined) return Promise.resolve(undefined);
        try {
          return Promise.resolve(JSON.parse(val));
        } catch (_) {
          return Promise.resolve(val);
        }
      },
      setItem: function (key, value) {
        if (
          globalThis.raycast &&
          typeof globalThis.raycast.storageSet === "function"
        ) {
          globalThis.raycast.storageSet(key, JSON.stringify(value));
        }
        return Promise.resolve();
      },
      removeItem: function (key) {
        if (
          globalThis.raycast &&
          typeof globalThis.raycast.storageDel === "function"
        ) {
          globalThis.raycast.storageDel(key);
        }
        return Promise.resolve();
      },
      clear: function () {
        if (
          globalThis.raycast &&
          typeof globalThis.raycast.storageClear === "function"
        ) {
          globalThis.raycast.storageClear();
        }
        return Promise.resolve();
      },
      allItems: function () {
        var rawJson =
          globalThis.raycast &&
          typeof globalThis.raycast.storageAll === "function"
            ? globalThis.raycast.storageAll()
            : "{}";
        try {
          var raw = JSON.parse(rawJson);
          var result = {};
          var keys = Object.keys(raw);
          for (var i = 0; i < keys.length; i++) {
            try {
              result[keys[i]] = JSON.parse(raw[keys[i]]);
            } catch (_) {
              result[keys[i]] = raw[keys[i]];
            }
          }
          return Promise.resolve(result);
        } catch (_) {
          return Promise.resolve({});
        }
      },
    },
  };

  // ── babelHelpers (for oxc CJS transform helpers) ─────────────────────────────

  globalThis.babelHelpers = {
    interopRequireDefault: function (obj) {
      return obj && obj.__esModule ? obj : { default: obj };
    },
    interopRequireWildcard: function (obj, nodeInterop) {
      if (!nodeInterop && obj && obj.__esModule) return obj;
      if (obj === null || (typeof obj !== "object" && typeof obj !== "function"))
        return { default: obj };
      var cache = new Map();
      var newObj = {};
      if (obj != null) {
        for (var key in obj) {
          if (key !== "default" && Object.prototype.hasOwnProperty.call(obj, key)) {
            newObj[key] = obj[key];
          }
        }
      }
      newObj.default = obj;
      return newObj;
    },
    objectSpread2: function (target) {
      for (var i = 1; i < arguments.length; i++) {
        var source = arguments[i];
        if (source != null) {
          for (var key in source) {
            if (Object.prototype.hasOwnProperty.call(source, key)) {
              target[key] = source[key];
            }
          }
        }
      }
      return target;
    },
    extends: function (target) {
      return Object.assign.apply(Object, [target].concat(Array.prototype.slice.call(arguments, 1)));
    },
    createClass: function (Constructor, protoProps, staticProps) {
      if (protoProps) Object.assign(Constructor.prototype, protoProps);
      if (staticProps) Object.assign(Constructor, staticProps);
      return Constructor;
    },
    defineProperty: function (obj, key, value) {
      if (key in obj) {
        Object.defineProperty(obj, key, {
          value: value,
          enumerable: true,
          configurable: true,
          writable: true,
        });
      } else {
        obj[key] = value;
      }
      return obj;
    },
    toPrimitive: function (input, hint) {
      if (typeof input !== "object" || input === null) return input;
      var prim = input[Symbol.toPrimitive];
      if (prim !== undefined) {
        var res = prim.call(input, hint || "default");
        if (typeof res !== "object") return res;
        throw new TypeError("@@toPrimitive must return a primitive value.");
      }
      return (hint === "string" ? String : Number)(input);
    },
    toPropertyKey: function (arg) {
      var key = globalThis.babelHelpers.toPrimitive(arg, "string");
      return typeof key === "symbol" ? key : String(key);
    },
    asyncToGenerator: function (fn) {
      return function () {
        var self = this,
          args = arguments;
        return new Promise(function (resolve, reject) {
          var gen = fn.apply(self, args);
          function step(key, arg) {
            try {
              var info = gen[key](arg);
              var value = info.value;
            } catch (error) {
              reject(error);
              return;
            }
            if (info.done) {
              resolve(value);
            } else {
              Promise.resolve(value).then(
                function (value) {
                  step("next", value);
                },
                function (err) {
                  step("throw", err);
                }
              );
            }
          }
          step("next", undefined);
        });
      };
    },
  };

  // ── Node.js built-in stubs ────────────────────────────────────────────────────

  var _pathModule = {
    sep: "/",
    join: function () {
      var args = Array.prototype.slice.call(arguments);
      var joined = args.filter(function (a) { return a != null && a !== ""; }).join("/");
      // Normalise multiple slashes but preserve a leading one
      return joined.replace(/\/+/g, "/").replace(/\/$/, "") || ".";
    },
    basename: function (p, ext) {
      var base = String(p).split("/").pop() || "";
      if (ext && typeof ext === "string" && base.length > ext.length && base.slice(-ext.length) === ext) {
        base = base.slice(0, base.length - ext.length);
      }
      return base;
    },
    dirname: function (p) {
      var s = String(p);
      var idx = s.lastIndexOf("/");
      if (idx < 0) return ".";
      if (idx === 0) return "/";
      return s.slice(0, idx);
    },
    extname: function (p) {
      var base = String(p).split("/").pop() || "";
      var dot = base.lastIndexOf(".");
      return dot > 0 ? base.slice(dot) : "";
    },
    resolve: function () {
      var args = Array.prototype.slice.call(arguments);
      var resolved = "";
      for (var i = args.length - 1; i >= 0; i--) {
        var part = String(args[i]);
        resolved = resolved ? part + "/" + resolved : part;
        if (part.charAt(0) === "/") break;
      }
      return resolved.replace(/\/+/g, "/");
    },
    isAbsolute: function (p) {
      return String(p).charAt(0) === "/";
    },
    normalize: function (p) {
      return String(p).replace(/\/+/g, "/");
    },
    relative: function (from, to) {
      // Very simplified: just return the `to` path
      return String(to);
    },
  };

  var _osModule = {
    homedir: function () {
      if (globalThis.raycast && typeof globalThis.raycast.homedir === "function") {
        return globalThis.raycast.homedir();
      }
      return "/home/user";
    },
    platform: function () {
      if (globalThis.raycast && typeof globalThis.raycast.platform === "function") {
        return globalThis.raycast.platform();
      }
      return "linux";
    },
    tmpdir: function () {
      return "/tmp";
    },
    hostname: function () {
      return "localhost";
    },
    EOL: "\n",
  };

  var _URLSearchParams = function URLSearchParams(init) {
    this._p = {};
    if (typeof init === "string" && init) {
      var s = init.charAt(0) === "?" ? init.slice(1) : init;
      s.split("&").forEach(function (pair) {
        if (!pair) return;
        var idx = pair.indexOf("=");
        var k = idx < 0 ? pair : pair.slice(0, idx);
        var v = idx < 0 ? "" : pair.slice(idx + 1);
        try { k = decodeURIComponent(k.replace(/\+/g, " ")); } catch (_) {}
        try { v = decodeURIComponent(v.replace(/\+/g, " ")); } catch (_) {}
        this._p[k] = v;
      }.bind(this));
    } else if (init && typeof init === "object") {
      var keys = Object.keys(init);
      for (var i = 0; i < keys.length; i++) {
        this._p[keys[i]] = String(init[keys[i]]);
      }
    }
  };
  _URLSearchParams.prototype.get = function (k) {
    return Object.prototype.hasOwnProperty.call(this._p, k) ? this._p[k] : null;
  };
  _URLSearchParams.prototype.set = function (k, v) { this._p[k] = String(v); };
  _URLSearchParams.prototype.has = function (k) {
    return Object.prototype.hasOwnProperty.call(this._p, k);
  };
  _URLSearchParams.prototype.append = function (k, v) { this._p[k] = String(v); };
  _URLSearchParams.prototype.delete = function (k) { delete this._p[k]; };
  _URLSearchParams.prototype.toString = function () {
    var self = this;
    return Object.keys(self._p).map(function (k) {
      return encodeURIComponent(k) + "=" + encodeURIComponent(self._p[k]);
    }).join("&");
  };
  _URLSearchParams.prototype.entries = function () {
    var self = this;
    var keys = Object.keys(self._p);
    var i = 0;
    return { next: function () {
      if (i < keys.length) {
        var k = keys[i++];
        return { value: [k, self._p[k]], done: false };
      }
      return { value: undefined, done: true };
    }};
  };

  var _URL = function URL(href, base) {
    this.href = String(href);
    var m = /^(\w+:)\/\/([^/?#]*)([^?#]*)(\?[^#]*)?(#.*)?$/.exec(this.href) || [];
    this.protocol = m[1] || "";
    this.host = m[2] || "";
    this.hostname = this.host.split(":")[0] || "";
    this.port = this.host.indexOf(":") >= 0 ? this.host.split(":")[1] : "";
    this.pathname = m[3] || "/";
    this.search = m[4] || "";
    this.hash = m[5] || "";
    this.origin = this.protocol + "//" + this.host;
    this.searchParams = new _URLSearchParams(this.search);
    this.toString = function () { return this.href; };
  };

  var _urlModule = {
    URL: _URL,
    URLSearchParams: _URLSearchParams,
  };

  var _querystringModule = {
    stringify: function (obj, sep, eq) {
      sep = sep || "&";
      eq = eq || "=";
      if (!obj) return "";
      return Object.keys(obj).map(function (k) {
        return encodeURIComponent(k) + eq + encodeURIComponent(obj[k]);
      }).join(sep);
    },
    parse: function (str, sep, eq) {
      sep = sep || "&";
      eq = eq || "=";
      var result = {};
      if (!str) return result;
      String(str).split(sep).forEach(function (pair) {
        var idx = pair.indexOf(eq);
        var k = idx < 0 ? pair : pair.slice(0, idx);
        var v = idx < 0 ? "" : pair.slice(idx + eq.length);
        if (k) {
          try { k = decodeURIComponent(k); } catch (_) {}
          try { v = decodeURIComponent(v); } catch (_) {}
          result[k] = v;
        }
      });
      return result;
    },
    escape: encodeURIComponent,
    unescape: decodeURIComponent,
  };

  // ── @raycast/utils shim ───────────────────────────────────────────────────────

  // In-memory fallback store for useLocalStorage when Rust bindings are absent.
  var _localStorageMemory = {};

  var raycastUtilsModule = (function () {
    // useCachedPromise: equivalent to usePromise (caching omitted for simplicity)
    function useCachedPromise(fn, args, options) {
      return usePromise(fn, args, options);
    }

    // useLocalStorage: returns { value, setValue, removeValue, isLoading, error }
    // Backed by raycast.storageGet/storageSet when available, otherwise in-memory.
    function useLocalStorage(key, defaultValue) {
      var initial = defaultValue !== undefined ? defaultValue : undefined;
      var stateArr = useState(initial);
      var value = stateArr[0];
      var setValue = stateArr[1];

      useEffect(function () {
        var raw;
        if (
          globalThis.raycast &&
          typeof globalThis.raycast.storageGet === "function"
        ) {
          raw = globalThis.raycast.storageGet(key);
        } else if (Object.prototype.hasOwnProperty.call(_localStorageMemory, key)) {
          raw = _localStorageMemory[key];
        }
        if (raw !== undefined && raw !== null) {
          try {
            setValue(JSON.parse(raw));
          } catch (_e) {
            setValue(raw);
          }
        }
      }, []);

      function set(newValue) {
        setValue(newValue);
        var serialised = JSON.stringify(newValue);
        if (
          globalThis.raycast &&
          typeof globalThis.raycast.storageSet === "function"
        ) {
          globalThis.raycast.storageSet(key, serialised);
        } else {
          _localStorageMemory[key] = serialised;
        }
      }

      function remove() {
        setValue(undefined);
        if (
          globalThis.raycast &&
          typeof globalThis.raycast.storageDel === "function"
        ) {
          globalThis.raycast.storageDel(key);
        } else {
          delete _localStorageMemory[key];
        }
      }

      return { value: value, setValue: set, removeValue: remove, isLoading: false, error: undefined };
    }

    function showFailureToast(titleOrOpts, opts) {
      if (typeof titleOrOpts === "string") {
        showToast({
          style: "failure",
          title: titleOrOpts,
          message: (opts && opts.message) ? String(opts.message) : "",
        });
      } else {
        showToast({
          style: "failure",
          title: (titleOrOpts && titleOrOpts.title) ? String(titleOrOpts.title) : "",
          message: (titleOrOpts && titleOrOpts.message) ? String(titleOrOpts.message) : "",
        });
      }
    }

    function getAvatarIcon(name) {
      // Return the first letter of the name as a simple avatar icon
      return (name && typeof name === "string" && name.length > 0)
        ? name[0].toUpperCase()
        : "?";
    }

    return {
      useCachedPromise: useCachedPromise,
      useLocalStorage: useLocalStorage,
      useFetch: useFetch,
      showFailureToast: showFailureToast,
      getAvatarIcon: getAvatarIcon,
    };
  })();

  // ── Node.js `crypto` shim ────────────────────────────────────────────────────
  // Provides createHash(algorithm).update(data).digest('hex') backed by the
  // native `raycast.cryptoHash` Rust binding.  Also exports randomUUID() as a
  // pure-JS fallback (Math.random based; not cryptographically secure).

  var _cryptoModule = (function () {
    function createHash(algorithm) {
      var chunks = [];
      var hasher = {
        update: function (data) {
          chunks.push(String(data));
          return hasher;
        },
        digest: function (encoding) {
          var combined = chunks.join("");
          var hex = globalThis.raycast.cryptoHash(algorithm, combined);
          if (!encoding || encoding === "hex") return hex;
          if (encoding === "base64") {
            // Convert hex → binary string → base64
            var bytes = "";
            for (var i = 0; i < hex.length; i += 2) {
              bytes += String.fromCharCode(parseInt(hex.slice(i, i + 2), 16));
            }
            return btoa(bytes);
          }
          return hex;
        },
      };
      return hasher;
    }

    function randomUUID() {
      return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(
        /[xy]/g,
        function (c) {
          var r = (Math.random() * 16) | 0;
          var v = c === "x" ? r : (r & 0x3) | 0x8;
          return v.toString(16);
        }
      );
    }

    function randomBytes(size) {
      var buf = new Uint8Array(size);
      for (var i = 0; i < size; i++) {
        buf[i] = (Math.random() * 256) | 0;
      }
      return buf;
    }

    return {
      createHash: createHash,
      randomUUID: randomUUID,
      randomBytes: randomBytes,
    };
  })();

  // ── require() — CJS module resolver ─────────────────────────────────────────

  function __raycast_require(name) {
    if (name === "@raycast/api" || name === "react") {
      return raycastApiModule;
    }
    if (name === "@raycast/utils") {
      return raycastUtilsModule;
    }
    if (name === "path" || name === "node:path") {
      return _pathModule;
    }
    if (name === "os" || name === "node:os") {
      return _osModule;
    }
    if (name === "url" || name === "node:url") {
      return _urlModule;
    }
    if (name === "querystring" || name === "node:querystring") {
      return _querystringModule;
    }
    if (name === "crypto" || name === "node:crypto") {
      return _cryptoModule;
    }
    if (
      name === "@raycast/api/jsx-runtime" ||
      name === "react/jsx-runtime" ||
      name === "react/jsx-dev-runtime"
    ) {
      return jsxRuntime;
    }
    // @oxc-project/runtime helpers (default helper loader mode)
    if (name.startsWith("@oxc-project/runtime/helpers/")) {
      var helperName = name.slice("@oxc-project/runtime/helpers/".length);
      // camelCase conversion (e.g. interop-require-default → interopRequireDefault)
      var camel = helperName.replace(/-([a-z])/g, function (_, c) {
        return c.toUpperCase();
      });
      if (globalThis.babelHelpers[camel]) {
        // Return as default export
        var h = globalThis.babelHelpers[camel];
        return { default: h, __esModule: true };
      }
      return { default: function () {}, __esModule: true };
    }
    // @babel/runtime helpers
    if (name.startsWith("@babel/runtime/helpers/")) {
      var helperName2 = name.slice("@babel/runtime/helpers/".length);
      var camel2 = helperName2.replace(/-([a-z])/g, function (_, c) {
        return c.toUpperCase();
      });
      if (globalThis.babelHelpers[camel2]) {
        var h2 = globalThis.babelHelpers[camel2];
        return { default: h2, __esModule: true };
      }
      return { default: function () {}, __esModule: true };
    }
    throw new Error("Cannot require module: " + name);
  }

  // ── Expose globals ───────────────────────────────────────────────────────────

  globalThis.__raycastBootstrap = __raycastBootstrap;
  globalThis.__raycast_require = __raycast_require;

  if (typeof globalThis.require === "undefined") {
    globalThis.require = __raycast_require;
  }

  // Expose React globally so classic JSX transform works without explicit import
  if (typeof globalThis.React === "undefined") {
    globalThis.React = React;
  }
})();
