//! T2.3 探针（正式版）：html5ever 的**脚本暂停 / 恢复 + document.write 插入点**协议。
//!
//! ## 背景（为什么这个探针这样写）
//! 初版探针只用低层 `Tokenizer` + `TokenSinkResult::Continue`，结果 `<script>`
//! 体内的 `<b>` 被当成真实标签——说明 **raw-text 状态不是 Tokenizer 自动进入的**。
//! 读源码后确认（html5ever 0.27 原文）：
//!
//! 1. `TokenSinkResult` 有四个变体，sink 可主动改变 tokenizer 状态：
//!    ```text
//!    pub enum TokenSinkResult<Handle> {
//!        Continue,
//!        Script(Handle),          // 要求暂停，把脚本节点交还驱动层
//!        Plaintext,
//!        RawData(states::RawKind) // 进入 Rcdata / Rawtext / ScriptData 状态
//!    }
//!    ```
//!    —— 处理 `<script>` 起始标签时返回 `RawData(ScriptData)` 即可进入脚本数据状态。
//! 2. `feed` 的返回类型是 `TokenizerResult<Handle> { Done, Script(Handle) }`，
//!    `Script(_)` 即「暂停点」，驱动层执行完脚本后可**再次 feed 恢复**。
//! 3. **html5ever 自带 driver 并未实现完整语义**，`src/driver.rs` 原文：
//!    ```text
//!    // FIXME: Properly support </script> somehow.
//!    while let TokenizerResult::Script(_) = self.tokenizer.feed(&mut self.input_buffer) {}
//!    ```
//!    它把 `Script(_)` 直接忽略后继续喂，即「不暂停、不执行脚本」。
//!
//! ## 本探针要证的结论
//! - (a) 增量 feed 成立（前缀 feed 后即产出 token）；
//! - (b) sink 返回 `RawData(ScriptData)` 后，script 体内 `<b>` **不再**被解析为标签；
//! - (c) `<script>` 结束处确实产生一个「暂停点」（`TokenizerResult::Script`）；
//! - (d) 在暂停点把 `document.write` 的产出 push 回 `BufferQueue`，其内容出现在
//!       `</script>` **之后**（插入点语义）；
//! - (e) 恢复后 tokenizer 回到 Data 状态，后续标签正常解析。

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
    TokenizerResult,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct Log {
    tokens: Vec<String>,
    script_bodies: Vec<String>,
}

struct Sink {
    log: Rc<RefCell<Log>>,
    in_script: bool,
}

impl TokenSink for Sink {
    /// 本 PoC 用 `()` 表示「脚本节点句柄」；真实实现里这里会是 DOM 节点句柄。
    type Handle = ();

    fn process_token(&mut self, token: Token, _line: u64) -> TokenSinkResult<Self::Handle> {
        let mut log = self.log.borrow_mut();
        match token {
            Token::TagToken(Tag { kind, name, .. }) => {
                let k = match kind {
                    TagKind::StartTag => "START",
                    TagKind::EndTag => "END",
                };
                log.tokens.push(format!("{k}({name})"));
                if name.as_ref() == "script" {
                    return match kind {
                        // 关键 1：进入脚本数据状态（raw text），体内标签不再被解析
                        TagKind::StartTag => {
                            self.in_script = true;
                            TokenSinkResult::RawData(RawKind::ScriptData)
                        }
                        // 关键 2：在 </script> 处请求「暂停，我要执行脚本」
                        TagKind::EndTag => {
                            self.in_script = false;
                            TokenSinkResult::Script(())
                        }
                    };
                }
            }
            Token::CharacterTokens(s) => {
                let t = s.to_string();
                if !t.trim().is_empty() {
                    log.tokens.push(format!("TEXT({})", t.trim()));
                    if self.in_script {
                        log.script_bodies.push(t);
                    }
                }
            }
            Token::CommentToken(s) => log.tokens.push(format!("COMMENT({s})")),
            Token::DoctypeToken(_) => log.tokens.push("DOCTYPE".to_string()),
            Token::NullCharacterToken => log.tokens.push("NUL".to_string()),
            Token::ParseError(e) => log.tokens.push(format!("ERR({e:?})")),
            Token::EOFToken => {}
        }
        TokenSinkResult::Continue
    }
}

fn main() {
    let log = Rc::new(RefCell::new(Log::default()));
    let sink = Sink {
        log: Rc::clone(&log),
        in_script: false,
    };
    let mut tok = Tokenizer::new(sink, TokenizerOpts::default());
    let mut input = BufferQueue::default();

    // ---------- 阶段 1：增量 feed 前缀 ----------
    input.push_back(StrTendril::from("<html><body><p>before"));
    match tok.feed(&mut input) {
        TokenizerResult::Done => {}
        TokenizerResult::Script(_) => println!("UNEXPECTED_PAUSE_IN_PREFIX"),
    }
    let snap1 = log.borrow().tokens.clone();
    println!("SNAPSHOT_AFTER_PREFIX = {snap1:?}");

    // ---------- 阶段 2：feed script 段 + 后缀，中途在暂停点注入 ----------
    input.push_back(StrTendril::from(
        "<script>var x = \"<b>not-a-tag</b>\";</script><i>after</i></p></body></html>",
    ));

    let mut pauses = 0usize;
    let mut writes = 0usize;
    loop {
        match tok.feed(&mut input) {
            TokenizerResult::Done => break,
            TokenizerResult::Script(_) => {
                pauses += 1;
                writes += 1;
                println!("SCRIPT_PAUSE #{pauses} → 执行脚本，document.write 注入内容");
                // 模拟 document.write：把产出插回输入流**队首**。
                // 注意：这里必须是 push_front 而不是 push_back —— 队列里还有尚未
                // 消费的输入（`</p></body></html>`），push_back 会把注入内容排到
                // 整段输入末尾，等价于「解析到文档结束后才插入」，不符合插入点语义。
                input.push_front(StrTendril::from("<u>written-by-script</u>"));
            }
        }
    }
    tok.end();

    let s = log.borrow();
    println!("SCRIPT_BODIES = {:?}", s.script_bodies);
    println!("FINAL_TOKENS = {:?}", s.tokens);

    // ---------- 断言 ----------
    let mut ok = true;
    let mut check = |cond: bool, tag: &str, pass: &str, fail: &str| {
        if cond {
            println!("ASSERT_PASS({tag}): {pass}");
        } else {
            println!("ASSERT_FAIL({tag}): {fail}");
            ok = false;
        }
    };

    check(
        snap1.contains(&"START(html)".to_string()),
        "a",
        "前缀 feed 后即产出 token —— 增量/可暂停成立",
        "prefix 未在首次 feed 产出",
    );
    check(
        !s.tokens.iter().any(|t| t == "START(b)"),
        "b",
        "script 体内 <b> 未被解析为标签 —— RawData(ScriptData) 生效",
        "script 体内 <b> 被误解析为标签",
    );
    check(
        pauses == 1,
        "c",
        "</script> 处产生 1 个暂停点 —— TokenSinkResult::Script 生效",
        "未产生暂停点",
    );
    let i_u = s.tokens.iter().position(|t| t == "START(u)");
    let i_script_end = s.tokens.iter().position(|t| t == "END(script)");
    check(
        matches!((i_script_end, i_u), (Some(e), Some(u)) if u > e),
        "d",
        "document.write 内容落在 </script> 之后 —— 插入点语义成立",
        "注入内容未落在 </script> 之后",
    );
    let i_i = s.tokens.iter().position(|t| t == "START(i)");
    check(
        i_i.is_some(),
        "e",
        "恢复后 </script> 后的 <i> 正常解析 —— tokenizer 回到 Data 状态",
        "恢复后后续标签未被解析",
    );

    println!("H5PROBE_{}", if ok { "OK" } else { "FAIL" });
}
