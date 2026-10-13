//! 分派算法（`web-events::dispatch`）。
//!
//! **纯函数 `plan_dispatch`**：算传播路径（target → root），按
//! `capture → at-target(capture) → at-target(bubble) → bubble` 生成 **owned `Vec<DispatchStep>`**
//! （借用在任何 `Realm::call` 之前释放 —— ADR-0016·D2 / ADR-0019·D2）。**不调 v8**。
//!
//! `DispatchStep.listeners` 是**候选 id 列表**——调用前须逐个 [`crate::target::ListenerRegistry::is_live`]
//! 查活性（分派期 `remove` 生效、`add` 不参与本次）。
//!
//! > at-target 拆**两段**模型以固定修订版 DOM Standard/WPT 锚定（登记 U-A；测试
//! > `listener_order_at_target_mixed_registration`）。

use web_dom::node::NodeId;
use web_dom::tree::NodeTree;

use crate::event::{Event, EventPhase};
use crate::listener::ListenerId;
use crate::target::ListenerRegistry;

/// 分派计划的一步：在 `target` 上按 `phase` 触发 `listeners`（候选 id）。
#[derive(Clone, Debug)]
pub struct DispatchStep {
    /// 本步目标节点。
    pub target: NodeId,
    /// 本步阶段。
    pub phase: EventPhase,
    /// 候选监听器 id（调用前须查活性）。
    pub listeners: Vec<ListenerId>,
}

/// 传播路径 `[target, parent, ..., root]`（到根为止）。
fn propagation_path(tree: &NodeTree, target: NodeId) -> Vec<NodeId> {
    let mut path = vec![target];
    let mut current = target;
    while let Some(parent) = tree.parent(current) {
        path.push(parent);
        if parent == tree.root() {
            break;
        }
        current = parent;
    }
    path
}

/// 某节点上指定 `capture` 类的候选监听器 id。
fn listener_ids(
    registry: &ListenerRegistry,
    node: NodeId,
    type_: &str,
    capture: bool,
) -> Vec<ListenerId> {
    registry
        .listeners(node, type_)
        .into_iter()
        .filter(|id| {
            registry
                .get(*id)
                .is_some_and(|listener| listener.options.capture == capture)
        })
        .collect()
}

/// 生成分派计划（owned，纯函数）。
///
/// 顺序：捕获（root → target 的父） → at-target（先 capture 段、后 bubble 段） → 冒泡
/// （target 的父 → root，**仅当** `event.bubbles`）。无监听器的节点不产出步骤。
#[must_use]
pub fn plan_dispatch(
    tree: &NodeTree,
    registry: &ListenerRegistry,
    target: NodeId,
    event: &Event,
) -> Vec<DispatchStep> {
    let path = propagation_path(tree, target);
    let mut steps = Vec::new();

    // 捕获阶段：root → ... → target 的父。
    for &node in path[1..].iter().rev() {
        let listeners = listener_ids(registry, node, &event.type_, true);
        if !listeners.is_empty() {
            steps.push(DispatchStep {
                target: node,
                phase: EventPhase::Capturing,
                listeners,
            });
        }
    }

    // at-target（两段）：capture 段、bubble 段。
    let at_capture = listener_ids(registry, target, &event.type_, true);
    if !at_capture.is_empty() {
        steps.push(DispatchStep {
            target,
            phase: EventPhase::AtTarget,
            listeners: at_capture,
        });
    }
    let at_bubble = listener_ids(registry, target, &event.type_, false);
    if !at_bubble.is_empty() {
        steps.push(DispatchStep {
            target,
            phase: EventPhase::AtTarget,
            listeners: at_bubble,
        });
    }

    // 冒泡阶段：target 的父 → root（仅当事件可冒泡）。
    if event.bubbles {
        for &node in &path[1..] {
            let listeners = listener_ids(registry, node, &event.type_, false);
            if !listeners.is_empty() {
                steps.push(DispatchStep {
                    target: node,
                    phase: EventPhase::Bubbling,
                    listeners,
                });
            }
        }
    }

    steps
}

#[cfg(test)]
mod tests {
    use super::{DispatchStep, plan_dispatch};
    use crate::event::{Event, EventInit, EventPhase};
    use crate::listener::ListenerOptions;
    use crate::target::ListenerRegistry;
    use engine_v8::WeakJsHandle;
    use web_dom::node::NodeId;
    use web_dom::tree::NodeTree;

    fn capture() -> ListenerOptions {
        ListenerOptions {
            capture: true,
            ..ListenerOptions::default()
        }
    }

    fn factory() -> (NodeTree, NodeId, NodeId, NodeId) {
        let mut tree = NodeTree::new();
        let root = tree.root();
        let body = tree.create_element("body", "html", vec![]);
        let target = tree.create_element("button", "html", vec![]);
        tree.append_child(root, body);
        tree.append_child(body, target);
        (tree, root, body, target)
    }

    fn flat(steps: &[DispatchStep]) -> Vec<(NodeId, EventPhase, usize)> {
        steps
            .iter()
            .map(|step| (step.target, step.phase, step.listeners.len()))
            .collect()
    }

    #[test]
    fn plan_dispatch_phase_order() {
        let (tree, root, body, target) = factory();
        let mut registry = ListenerRegistry::new();
        let r_cap = registry.add(root, "click", WeakJsHandle::new(1, 0), capture());
        let b_cap = registry.add(body, "click", WeakJsHandle::new(2, 0), capture());
        let t_cap = registry.add(target, "click", WeakJsHandle::new(3, 0), capture());
        let t_bub = registry.add(
            target,
            "click",
            WeakJsHandle::new(4, 0),
            ListenerOptions::default(),
        );
        let b_bub = registry.add(
            body,
            "click",
            WeakJsHandle::new(5, 0),
            ListenerOptions::default(),
        );
        let r_bub = registry.add(
            root,
            "click",
            WeakJsHandle::new(6, 0),
            ListenerOptions::default(),
        );

        let event = Event::new(
            "click",
            EventInit {
                bubbles: true,
                ..EventInit::default()
            },
        );
        let steps = plan_dispatch(&tree, &registry, target, &event);

        let expected = vec![
            (root, EventPhase::Capturing, 1),
            (body, EventPhase::Capturing, 1),
            (target, EventPhase::AtTarget, 1),
            (target, EventPhase::AtTarget, 1),
            (body, EventPhase::Bubbling, 1),
            (root, EventPhase::Bubbling, 1),
        ];
        assert_eq!(
            flat(&steps),
            expected,
            "capture→at-target(cap→bub)→bubble 顺序"
        );
        assert_eq!(steps[0].listeners, vec![r_cap]);
        assert_eq!(steps[1].listeners, vec![b_cap]);
        assert_eq!(steps[2].listeners, vec![t_cap], "at-target capture 段");
        assert_eq!(steps[3].listeners, vec![t_bub], "at-target bubble 段");
        assert_eq!(steps[4].listeners, vec![b_bub]);
        assert_eq!(steps[5].listeners, vec![r_bub]);
    }

    #[test]
    fn listener_order_at_target_mixed_registration() {
        // U-A 锚定：at-target 上 capture/bubble 拆两段（先 cap 段、后 bub 段）。
        let (tree, _root, _body, target) = factory();
        let mut registry = ListenerRegistry::new();
        let bubble_first = registry.add(
            target,
            "click",
            WeakJsHandle::new(1, 0),
            ListenerOptions::default(),
        );
        let cap_second = registry.add(target, "click", WeakJsHandle::new(2, 0), capture());
        let event = Event::new("click", EventInit::default());
        let steps = plan_dispatch(&tree, &registry, target, &event);
        assert_eq!(steps.len(), 2, "at-target 应拆为两段");
        assert_eq!(steps[0].phase, EventPhase::AtTarget);
        assert_eq!(steps[0].listeners, vec![cap_second], "capture 段先");
        assert_eq!(steps[1].listeners, vec![bubble_first], "bubble 段后");
    }

    #[test]
    fn plan_dispatch_stops_at_root() {
        let (tree, root, _body, target) = factory();
        let mut registry = ListenerRegistry::new();
        registry.add(root, "click", WeakJsHandle::new(1, 0), capture());
        registry.add(
            root,
            "click",
            WeakJsHandle::new(2, 0),
            ListenerOptions::default(),
        );
        let event = Event::new(
            "click",
            EventInit {
                bubbles: true,
                ..EventInit::default()
            },
        );
        let steps = plan_dispatch(&tree, &registry, target, &event);
        // 只有 root 两条（capture + bubble）；不得越过 root。
        assert_eq!(steps.len(), 2);
        assert!(
            steps.iter().all(|step| step.target == root),
            "路径不得越过根"
        );
        assert_eq!(steps[0].phase, EventPhase::Capturing);
        assert_eq!(steps[1].phase, EventPhase::Bubbling);

        // 不冒泡 ⇒ 无 bubble 步骤。
        let no_bubble = Event::new("click", EventInit::default());
        let steps = plan_dispatch(&tree, &registry, target, &no_bubble);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].phase, EventPhase::Capturing);
    }

    #[test]
    fn listener_removed_during_dispatch_not_invoked() {
        // ADR-0019·D2：分派期 remove 尚未调用者 ⇒ 调用时跳过。
        let (tree, _root, body, target) = factory();
        let mut registry = ListenerRegistry::new();
        let doomed = registry.add(
            body,
            "click",
            WeakJsHandle::new(1, 0),
            ListenerOptions::default(),
        );
        let keeper = registry.add(
            body,
            "click",
            WeakJsHandle::new(2, 0),
            ListenerOptions::default(),
        );
        let event = Event::new(
            "click",
            EventInit {
                bubbles: true,
                ..EventInit::default()
            },
        );
        let steps = plan_dispatch(&tree, &registry, target, &event);
        let body_bubble = steps
            .iter()
            .find(|step| step.phase == EventPhase::Bubbling && step.target == body)
            .expect("应有 body 冒泡步骤");
        assert!(body_bubble.listeners.contains(&doomed));

        // 分派进行中移除 doomed。
        registry.remove(body, "click", WeakJsHandle::new(1, 0), false);
        let invoked: Vec<_> = body_bubble
            .listeners
            .iter()
            .copied()
            .filter(|id| registry.is_live(*id))
            .collect();
        assert_eq!(invoked, vec![keeper], "已移除者应被跳过");
    }

    #[test]
    fn listener_added_during_dispatch_not_invoked() {
        // ADR-0019·D2：分派期新增 ⇒ 不参与本次（计划是 owned 快照）。
        let (tree, _root, body, target) = factory();
        let mut registry = ListenerRegistry::new();
        let existing = registry.add(
            body,
            "click",
            WeakJsHandle::new(1, 0),
            ListenerOptions::default(),
        );
        let event = Event::new(
            "click",
            EventInit {
                bubbles: true,
                ..EventInit::default()
            },
        );
        let steps = plan_dispatch(&tree, &registry, target, &event);
        let body_bubble = steps
            .iter()
            .find(|step| step.phase == EventPhase::Bubbling && step.target == body)
            .expect("应有 body 冒泡步骤");
        let snapshot = body_bubble.listeners.clone();

        // 分派进行中新增。
        let late = registry.add(
            body,
            "click",
            WeakJsHandle::new(9, 0),
            ListenerOptions::default(),
        );
        assert!(registry.is_live(late), "新监听器本身是活的……");
        assert!(!snapshot.contains(&late), "……但不在本次计划快照内");
        assert_eq!(snapshot, vec![existing]);
    }
}
