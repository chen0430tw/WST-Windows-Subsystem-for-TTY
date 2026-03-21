# 修复终端复制文本阶梯效果

## 问题描述

在 Windows Terminal 中使用 WST 时，从 TUI 界面复制命令输出（如 `dir` 命令）后粘贴到文本文件，会出现**阶梯状排列**效果：

```
第一行文本
  第二行文本（缩进）
    第三行文本（更多缩进）
```

**关键特征**：此问题仅出现在**复制粘贴**时，屏幕显示完全正常。

## 根本原因

### 技术分析

1. **ratatui 渲染机制**：ratatui 的 Widget（如 `Paragraph`）在渲染时会填充整个分配的区域，包括设置背景色到每个单元格

2. **Windows Terminal 行为**：当用户选择文本进行复制时，Windows Terminal 会读取终端缓冲区的所有单元格，包括：
   - 有实际字符的单元格
   - 只有背景色的空白单元格

3. **问题链条**：
   ```
   ratatui 填充背景色 → Windows Terminal 识别为有内容 → 复制时包含空格 → 粘贴出现阶梯
   ```

4. **为什么屏幕显示正常但复制异常**：
   - 显示时：背景色单元格与默认背景视觉相同
   - 复制时：终端将这些单元格解释为空格字符

## 解决方案：方案2 - 直接操作终端 Buffer

### 思路

绕过 ratatui 的高级 Widget，直接操作终端缓冲区，**只写入实际文本字符**，不填充空白区域。

### 代码实现

**文件**: `apps/wst-ui/src/main.rs` - `draw_ui()` 函数

```rust
// 获取终端缓冲区的直接访问权限
let buf = f.buffer_mut();

// 逐字符写入，不填充宽度
for line in state.output.iter().skip(start).take(end - start) {
    let expanded_text: String = line.text.replace('\t', "        ").trim_end().to_string();

    let mut col = area.x;
    for ch in expanded_text.chars() {
        if col < area.x + area.width {
            buf.get_mut(col, area.y + y_offset)
                .set_char(ch)
                .set_style(style.clone());
            col += 1;
        }
    }
    y_offset += 1;
}
```

### 关键点

1. **使用 `f.buffer_mut()`**：获取底层的终端缓冲区可变引用

2. **逐字符写入**：`buf.get_mut(col, row).set_char(ch)` - 只在有字符的位置写入

3. **不填充宽度**：循环结束后不继续填充空格到行尾

4. **先清除区域**：`f.render_widget(ratatui::widgets::Clear, area)` - 确保干净的状态

### 对比

| 方法 | 代码复杂度 | 复制效果 | 功能 |
|------|-----------|---------|------|
| Paragraph Widget | 低 | 有阶梯 | 完整 |
| 直接 Buffer 操作 | 中 | 正常 | 完整 |

## 验证

测试命令：
```bash
.\target\release\wst-ui.exe
:backend cmd
dir
# 选择并复制输出
```

预期结果：粘贴后文本整齐对齐，无阶梯效果

## 相关文件

- `apps/wst-ui/src/main.rs` - UI 渲染逻辑
- `crates/wst-backend/src/lib.rs` - 后端输出处理（`\r` 字符处理）

## 参考

- ratatui 文档: https://docs.rs/ratatui/
- Windows Terminal 文本选择机制
