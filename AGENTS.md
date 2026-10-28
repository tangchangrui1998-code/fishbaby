# 项目协作约定

- 每轮改动完成后，运行与改动相关的检查，通过后提交并推送当前工作分支。
- `main` 保留公共基线，`mac` 为 macOS 分支，`windows` 为 Windows 分支；公共改动提交到 `main` 后同步到相关平台分支。
- 不提交依赖目录、构建产物、用户书籍或本地阅读数据。安装包保存在忽略的 releases/ 目录或 GitHub Actions artifacts。
- 产品范围和验收标准见 docs/PRODUCT_PLAN.md。
