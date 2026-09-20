# ADR-0006:CLI 进程内分发

## 状态
生效

## 背景
LLM 的 shell 工具执行 `neomind xxx` 时,若走子进程会依赖 PATH 里的 CLI 二进制 —— 与运行中服务器存在版本漂移,且 Windows 下 DLL 搜索路径易出问题。

## 决策
shell 工具拦截 `neomind` 前缀命令,经 `neomind_cli_ops::dispatch(argv)` 在服务器进程内执行,返回结构化 CliResponse;仅 Serve/Prompt/Chat/Logs/Health 等少数子命令回退子进程。

## 后果
+ 零版本漂移;结构化结果(含 suggestion 恢复提示)直接喂给 LLM;跨平台一致。
- CLI 代码因此链接进主二进制,体积与编译面增大。
