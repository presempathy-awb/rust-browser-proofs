//! Two-window Web Locks proof for the experimental, local-only `IdbVfs`.

#![cfg(all(target_arch = "wasm32", feature = "idb-vendor-spike"))]

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen(inline_js = r#"
const crossTabMessages = [];

window.addEventListener('message', (event) => {
  if (typeof event.data === 'string') {
    crossTabMessages.push(event.data);
  }
});

export function resetCrossTabMessages() {
  crossTabMessages.length = 0;
}

export function takeCrossTabMessage() {
  return crossTabMessages.shift();
}

export function sleepCrossTab(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function openCrossTabPopup(name) {
  const popup = window.open('about:blank', name, 'popup,width=320,height=240');
  if (!popup) {
    throw new Error('browser blocked the cross-tab popup');
  }
  if (!popup.opener) {
    throw new Error('cross-tab popup has no opener');
  }
  const popupScript = `
    window.addEventListener('message', (event) => {
      const lockName = event.data;
      if (typeof lockName !== 'string') return;
      let release;
      const held = new Promise((resolve) => { release = resolve; });
      navigator.locks.request(lockName, { mode: 'exclusive', ifAvailable: true }, (lock) => {
        if (lock === null) {
          window.opener.postMessage('popup:lock-unavailable', '*');
          return undefined;
        }
        window.opener.postMessage('popup:idb-lock-held', '*');
        return held;
      }).catch((error) => {
        window.opener.postMessage('popup:error:' + String(error), '*');
      });
    });
  `;
  popup.document.open();
  popup.document.write('<!doctype html><script>' + popupScript + '</script>');
  popup.document.close();
  return popup;
}

export function startPopupLock(popup, lockName) {
  popup.postMessage(lockName, '*');
}

export function probeCrossTabLock(lockName) {
  navigator.locks.request(lockName, { mode: 'exclusive', ifAvailable: true }, (lock) => {
    crossTabMessages.push(lock === null ? 'probe:idb-lock-contended' : 'probe:idb-lock-acquired');
  });
}

export function closeCrossTabPopup(popup) {
  popup.close();
}
"#)]
extern "C" {
    fn resetCrossTabMessages();
    fn takeCrossTabMessage() -> Option<String>;
    fn sleepCrossTab(ms: i32) -> js_sys::Promise;
    fn openCrossTabPopup(name: &str) -> JsValue;
    fn startPopupLock(popup: &JsValue, lock_name: &str);
    fn probeCrossTabLock(lock_name: &str);
    fn closeCrossTabPopup(popup: &JsValue);
}

async fn wait_for_message() -> String {
    for _ in 0..200 {
        if let Some(message) = takeCrossTabMessage() {
            return message;
        }
        JsFuture::from(sleepCrossTab(50)).await.unwrap();
    }
    panic!("cross-tab driver sent no message within 10 seconds");
}

#[wasm_bindgen_test]
async fn idb_web_locks_are_contended_across_tabs_then_release_after_popup_close() {
    let root = format!("cross-tab-{}", js_sys::Date::now());
    let lock_name = format!("pagedb-idb-vfs:{root}:lock:/writer");
    resetCrossTabMessages();

    let popup = openCrossTabPopup(&format!("pagedb-idb-{root}"));
    startPopupLock(&popup, &lock_name);
    assert_eq!(wait_for_message().await, "popup:idb-lock-held");

    probeCrossTabLock(&lock_name);
    assert_eq!(wait_for_message().await, "probe:idb-lock-contended");

    closeCrossTabPopup(&popup);
    let mut acquired = false;
    for _ in 0..100 {
        probeCrossTabLock(&lock_name);
        let message = wait_for_message().await;
        match message.as_str() {
            "probe:idb-lock-acquired" => {
                acquired = true;
                break;
            }
            "probe:idb-lock-contended" => {
                JsFuture::from(sleepCrossTab(50)).await.unwrap();
            }
            _ => panic!("unexpected post-close lock probe: {message}"),
        }
    }
    assert!(acquired, "Web Lock was not released after the popup closed");
}
