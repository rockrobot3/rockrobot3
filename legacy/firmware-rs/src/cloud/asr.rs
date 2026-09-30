//! asr：语音识别客户端（占位）。
//! 流程契约：asr_begin 预握手 → 流式 chunk 上传 → finish 收文本；
//! 服务端不支持 chunked，需 Content-Length 预声明 + 说完提前结束补零。
