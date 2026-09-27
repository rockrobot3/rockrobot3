/* log 模块实现：esp_log vprintf 钩子 + RAM 环形缓冲 + 镜像回调 */
#include "applog.h"
#include "log_config.h"
#include <stdio.h>
#include <stdarg.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "esp_log.h"
#include "esp_timer.h"

static char          s_ring[LOG_MAX_LINES][LOG_LINE_MAX];
static int           s_head;                     /* 下一条写入位置 */
static int           s_count;
static portMUX_TYPE  s_mux = portMUX_INITIALIZER_UNLOCKED;
static vprintf_like_t s_orig;
static void        (*s_mirror)(const char *line);

/* 去掉行尾换行后存入环形缓冲；持锁调用，镜像在锁外回调 */
static void ring_push_locked(char *line)
{
    int n = strlen(line);
    while (n > 0 && (line[n - 1] == '\n' || line[n - 1] == '\r'))
        line[--n] = 0;
    if (n == 0) return;
    if (n >= LOG_LINE_MAX) {                    /* 超长截断，末尾标记 */
        n = LOG_LINE_MAX - 2;
        line[n] = '~';
        line[n + 1] = 0;
    }
    strlcpy(s_ring[s_head], line, LOG_LINE_MAX);
    s_head = (s_head + 1) % LOG_MAX_LINES;
    if (s_count < LOG_MAX_LINES) s_count++;
}

/* esp_log 钩子：串口照常输出，同时入缓冲 */
static int log_vprintf(const char *fmt, va_list args)
{
    char line[LOG_LINE_MAX];
    int len = vsnprintf(line, sizeof(line), fmt, args);

    void (*mirror)(const char *) = NULL;
    portENTER_CRITICAL(&s_mux);
    if (len > 0) { ring_push_locked(line); mirror = s_mirror; }
    portEXIT_CRITICAL(&s_mux);
    if (mirror) mirror(line);
    return (s_orig) ? s_orig(fmt, args) : len;
}

void log_init(void)
{
    if (s_orig) return;                         /* 防重复接管 */
    s_orig = esp_log_set_vprintf(log_vprintf);
}

void log_writef(const char *fmt, ...)
{
    char line[LOG_LINE_MAX];
    int64_t us = esp_timer_get_time();
    int n = snprintf(line, sizeof(line), "[%6lld.%03d] ",
                     (long long)(us / 1000000), (int)((us / 1000) % 1000));
    va_list args;
    va_start(args, fmt);
    vsnprintf(line + n, sizeof(line) - n, fmt, args);
    va_end(args);

    void (*mirror)(const char *) = NULL;
    portENTER_CRITICAL(&s_mux);
    ring_push_locked(line);
    mirror = s_mirror;
    portEXIT_CRITICAL(&s_mux);
    if (mirror) mirror(line);
    printf("%s\n", line);                       /* 串口同步可见（printf 不走 log 钩子，无递归） */
}

int log_count(void)
{
    return s_count;
}

void log_set_mirror(void (*fn)(const char *line))
{
    portENTER_CRITICAL(&s_mux);
    s_mirror = fn;
    portEXIT_CRITICAL(&s_mux);
}

/* JSON 字符串转义（处理引号/反斜杠/换行等控制字符） */
static int json_escape(char *out, int sz, const char *src)
{
    int o = 0;
    for (const char *p = src; *p && o < sz - 7; p++) {
        unsigned char c = (unsigned char)*p;
        if (c == '"' || c == '\\') { out[o++] = '\\'; out[o++] = c; }
        else if (c == '\n') { out[o++] = '\\'; out[o++] = 'n'; }
        else if (c == '\r') { out[o++] = '\\'; out[o++] = 'r'; }
        else if (c == '\t') { out[o++] = '\\'; out[o++] = 't'; }
        else if (c < 0x20)  { o += snprintf(out + o, sz - o, "\\u%04x", c); }
        else out[o++] = c;
    }
    out[o] = 0;
    return o;
}

int log_dump_json(char *out, int out_sz, int max_lines)
{
    int o = 0;
    o += snprintf(out + o, out_sz - o, "[");
    portENTER_CRITICAL(&s_mux);
    int n = (max_lines > s_count) ? s_count : max_lines;
    for (int i = 0; i < n; i++) {               /* 新的在前 */
        int idx = (s_head - 1 - i + LOG_MAX_LINES * 2) % LOG_MAX_LINES;
        char esc[LOG_LINE_MAX * 2 + 8];
        json_escape(esc, sizeof(esc), s_ring[idx]);
        o += snprintf(out + o, out_sz - o, "%s\"%s\"", i ? "," : "", esc);
        if (o >= out_sz - 2) break;
    }
    portEXIT_CRITICAL(&s_mux);
    snprintf(out + o, out_sz - o, "]");
    return o;
}
