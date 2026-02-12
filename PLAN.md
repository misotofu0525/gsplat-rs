# gsplat-rs 开发计划（增强版）

> 目标：构建一个可复用的 Rust + wgpu Gaussian Splatting 渲染库，优先 iOS/Android，后续无缝复用到 Web。

## 1. 本次计划增强点（相对上一版）

本计划把项目明确为 **GPU 数据管线工程**，不是单纯 shader 工程。  
核心优化目标固定为四项：**可见集合规模、排序成本、过度绘制（overdraw）、带宽压力**。

新增的关键改进：

1. 建立逐帧性能预算与 KPI（可量化验收）。
2. 管线模块化：排序与混合策略可插拔，不和主渲染器耦死。
3. 加入“兼容优先 / 移动优先”双模式与自动降级策略。
4. 引入离线打包与分块格式，避免运行时直接吃原始 PLY 的解析/带宽成本。
5. 为移动端定义明确的观测指标（而不只看 FPS）。
6. 对近两年论文给出“可工程化优先级”与“是否纳入 v1”结论。

## 2. 固定目标与边界

### 2.1 产品目标

1. v1 目标平台：iOS / Android（优先），Web 次阶段复用同内核。
2. v1 功能边界：静态场景查看 + 相机控制 + 性能统计。
3. v1 输入格式：PLY（标准 3DGS 导出字段）。
4. v1 渲染策略：双路径  
   - `SortedAlpha`（默认，兼容优先）  
   - `SortFree`（性能优先，近似混合）
5. 对外集成方式：C ABI + Swift/Kotlin 封装。

### 2.2 非目标（v1 明确不做）

1. 不做训练流程与优化器。
2. 不做动态高斯/动画高斯。
3. 不做编辑器能力（编辑、涂改、重拓扑等）。
4. 不做 WebGL 回退路径（Web 仅 WebGPU）。

## 3. 逐帧预算与验收 KPI（720p@60fps）

帧预算：16.6ms（中端移动 SoC 目标）。

建议预算上限（v1 验收线）：

1. 可见集预处理（Cull + Key）：<= 2.5ms
2. 排序（GPU radix / fallback）：<= 4.0ms
3. 光栅与混合：<= 7.0ms
4. 其他 CPU/提交开销：<= 3.1ms

强制采集指标：

1. `visible_count`
2. `drawn_count`
3. `sort_ms`
4. `preprocess_ms`
5. `raster_ms`
6. `frame_ms`
7. `overdraw_proxy`（每像素累计 alpha 或绘制层数近似）
8. `bandwidth_proxy`（读写字节估算）

> 验收不是“单次跑到 60fps”，而是持续 30 分钟下 P50/P95 稳定达标。

## 4. 架构设计（可替换、可迭代）

## 4.1 Workspace 拆分

1. `gsplat-core`：数学、相机、数据结构、错误模型
2. `gsplat-io-ply`：PLY 解析、字段校验、属性转换
3. `gsplat-format`：离线紧凑格式（chunk + 索引 + 元数据）
4. `gsplat-render-wgpu`：帧图、资源池、渲染 pass
5. `gsplat-sort`：排序后端抽象（GPU/CPU/增量）
6. `gsplat-ffi-c`：稳定 C ABI
7. `apps/ios-demo` `apps/android-demo` `apps/web-demo`：平台示例

## 4.2 渲染帧图（固定顺序）

1. `Pass A - Visibility & Projection`  
   输入：原始 splat 缓冲  
   输出：可见索引、深度 key、屏幕空间派生参数
2. `Pass B - Sort`（仅 SortedAlpha）  
   输出：排序索引
3. `Pass C - Draw`  
   Instanced quad（wgpu/WebGPU 约束下不使用 geometry shader）
4. `Pass D - Blend/Compose`  
   - Sorted：标准 premultiplied alpha  
   - SortFree：加权近似融合

## 4.3 排序/混合策略接口

```text
trait SortBackend {
  fn prepare(...)
  fn sort(...)
}

trait BlendBackend {
  fn requires_sort() -> bool
  fn render(...)
}
```

默认实现：

1. `SortBackendGpuRadix`
2. `SortBackendCpuFallback`
3. `BlendBackendSortedAlpha`
4. `BlendBackendWeighted`（SortFree）

## 5. 数据格式与资产管线

### 5.1 v1 输入（运行时）

支持字段：

1. 位置：`x/y/z`
2. 不透明度：`opacity`（运行时转 `alpha = sigmoid(opacity)`）
3. 尺度：`scale_0..2`（运行时 `exp` 还原）
4. 旋转：`rot_0..3`
5. 颜色/SH：`f_dc_0..2`（v1 必选），`f_rest_*`（v1 可选降级）

### 5.2 v1.1 输入优化（离线工具）

新增 `gsplat pack`：

1. PLY -> 紧凑二进制（SoA + 16-byte 对齐）
2. 支持 chunk 切分（为流式加载/LOD 预备）
3. 写入统计元数据（边界盒、splat 计数、属性范围）

## 6. 跨平台集成策略

1. iOS：`CAMetalLayer` + C ABI + Swift wrapper
2. Android：`ANativeWindow` + C ABI + JNI/Kotlin wrapper
3. Web：`wasm + webgpu`，共享核心渲染逻辑

统一 API（冻结到 v0.1）：

1. context 创建销毁
2. scene 加载（path/memory）
3. camera 设置
4. render mode 切换
5. resize
6. render_frame
7. 获取 stats / error

## 7. 论文路线落地优先级（截至 2026-02-12）

### P1（主线优先：训练无强绑定 / Drop-in 优化）

1. FlashGS（CVPR 2025）：优先落地“减少冗余 pair + 任务均衡”类纯渲染优化
2. SeeLe / Voyager：优先吸收可见集过滤、时间相关缓存、预过滤等无需改训练的策略
3. StopThePop：优先评估其排序一致性思路中可独立移植的部分（不改变训练目标）
4. 排序后端工程优化：GPU radix、CPU fallback、增量更新（不要求资产重训）

### P2（实验分支：可能存在 forward model 偏差）

1. Sort-free WSR（ICLR 2025）：作为可选近似模式，不作为默认主线质量路径
2. StochasticSplats：作为可调质量的实验后端，重点评估时间稳定性
3. Hybrid Transparency / Duplex-GS：作为近似透明合成路线，先做研究验证

### P3（强配套或硬件特化：后置）

1. Mobile-GS（ICLR 2026）完整方案：涉及表示/压缩/增强的配套训练假设，后置
2. GauRast / Neo 的硬件特化部分：仅吸收算法思想，不依赖专用硬件实现

### 7.1 论文方案准入门槛（新增）

任何候选方案进入主线前，必须先满足：

1. `drop-in`：可直接渲染标准 3DGS 资产，不要求重训或额外网络
2. `quality-safe`：与 `SortedAlpha` 参考路径对比，通过一致性基线
3. `fallback-safe`：运行时可无损回退到参考路径
4. `metadata-clear`：若需要配套训练，必须在文档与资产元数据中显式声明

## 8. 里程碑（12 周）

1. W1-W2：workspace 架构、CI、PLY parser、基础数据模型
2. W3-W4：wgpu 最小可渲染链路 + iOS/Android surface 打通
3. W5-W6：SortedAlpha 全流程（预处理 + 排序 + 渲染）
4. W7：C ABI 冻结 + Swift/Kotlin 封装初版
5. W8-W9：主线性能优化（可见集/overdraw/带宽）+ 质量回归基线
6. W10：Web 复用验证（wasm + WebGPU）
7. W11：可选实验后端接入（SortFree/Hybrid/Stochastic，至少一种）
8. W12：回归测试、文档、发布 `v0.1.0`（默认仅承诺参考路径）

## 9. 风险与回退策略

1. 风险：移动 GPU 上排序抖动  
   回退：降低排序频率 + 动态降分辨率 + CPU fallback（优先保持 SortedAlpha）
2. 风险：SortFree 透明重叠伪影  
   回退：场景级切回 SortedAlpha
3. 风险：PLY 解析与加载过慢  
   回退：强制使用离线打包格式
4. 风险：WebGPU 设备兼容性差异  
   回退：能力探测 + feature gating（不提供 WebGL fallback）
5. 风险：论文方案与资产训练假设不匹配，导致“能渲染但效果偏差”  
   回退：默认禁用该后端，仅允许实验开关启用，并回归到 `SortedAlpha`

## 10. 测试矩阵与发布门槛

### 10.1 测试矩阵

1. 数据规模：100k / 500k / 1M / 3M splats
2. 分辨率：720p / 1080p
3. 模式：SortedAlpha / SortFree
4. 平台：iOS / Android / Web

### 10.2 发布门槛（v0.1.0）

1. iOS + Android：720p 场景 P95 >= 55fps，P50 >= 60fps
2. 连续运行 30 分钟无崩溃、无显著内存增长
3. C ABI 头文件稳定并通过示例工程集成
4. `SortedAlpha` 作为唯一质量承诺路径通过一致性测试
5. 任何实验后端均不作为 v0.1 质量承诺，仅提供显式开关与对比报告

## 11. 下一步执行建议

1. 先把 `gsplat-core` / `gsplat-io-ply` / `gsplat-render-wgpu` 的骨架建好。
2. 第一周内完成 KPI 采集埋点，不等到“优化阶段”再补。
3. 先交付 `SortedAlpha` 正确性基线，再上 `SortFree`，避免两条线同时调试。
