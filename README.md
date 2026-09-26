# QueenX Kernel

QueenX 是一个从零实现的内核，使用 Rust 编写，基于 Asterinas 框内核（Framekernel）架构与范式进行设计与开发。它目前是实验性的，并在此基础上设计了一些独特的功能与特性。

> **English version**: [README.en.md](README.en.md)

## 项目入口

- **人类开发者 / 阅读者**：请前往 [`docs/explain/`](docs/explain/) 目录查阅开发与架构指引文档。
- **AI / Agent 开发者**：请阅读项目根目录下的 [`AGENTS.md`](AGENTS.md) 并严格遵循其中的规则。阅读后可根据其导览阅读更多其他关联文档。

## 开发模式说明

本项目采用**人在回路（human-in-the-loop）的人机协作模式**开发：人负责方向规划、架构决策与最终审查，AI / Agent 负责具体的编码实施。这一模式带来了可观的开发效率，也让代码可能带有机器生成源常见的痕迹与瑕疵。若你发现了任何可疑或错误的代码，欢迎指出与贡献。

## 反馈与协作

欢迎提交问题报告与贡献：

- **主仓库**：[Gitee](https://gitee.com/AnferLagbu/QueenX)
- **镜像仓库**：[GitHub](https://github.com/AnferLagbu/QueenX)

无论是问题报告还是贡献提交，我都会关注，并由衷感谢你对本项目的关注与贡献。