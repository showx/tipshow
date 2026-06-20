package cmd

import (
	"context"
	"fmt"
	"time"

	"github.com/shengsheng/tipshow/internal/output"
	"github.com/shengsheng/tipshow/internal/runner"
	"github.com/shengsheng/tipshow/internal/tools"
	"github.com/spf13/cobra"
)

// wrap 定义：透传外部监控命令，按层级分组
type wrapDef struct {
	Use         string
	Short       string
	Level       tools.Level
	Binary      string
	DefaultArgs []string
	Alt         []string // 备选二进制
	LinuxOnly   bool
}

var wrapDefs = []wrapDef{
	// 中级
	{Use: "htop", Short: "交互式进程监控", Level: tools.LevelIntermediate, Binary: "htop"},
	{Use: "iostat", Short: "CPU/磁盘 IO 统计", Level: tools.LevelIntermediate, Binary: "iostat", DefaultArgs: []string{"-xz", "1", "3"}},
	{Use: "iotop", Short: "按进程磁盘 IO", Level: tools.LevelIntermediate, Binary: "iotop", DefaultArgs: []string{"-o"}},
	{Use: "ifconfig", Short: "网络接口", Level: tools.LevelIntermediate, Binary: "ifconfig"},
	{Use: "ip", Short: "现代网络工具", Level: tools.LevelIntermediate, Binary: "ip", DefaultArgs: []string{"addr"}},
	{Use: "ss", Short: "Socket 统计", Level: tools.LevelIntermediate, Binary: "ss", DefaultArgs: []string{"-tunap"}},
	{Use: "netstat", Short: "网络连接", Level: tools.LevelIntermediate, Binary: "netstat", DefaultArgs: []string{"-anv"}},
	{Use: "lsof", Short: "打开文件/端口", Level: tools.LevelIntermediate, Binary: "lsof", DefaultArgs: []string{"-i", "-P"}},
	// 高级
	{Use: "sar", Short: "系统活动报告", Level: tools.LevelAdvanced, Binary: "sar", DefaultArgs: []string{"1", "5"}},
	{Use: "pidstat", Short: "按 PID 统计", Level: tools.LevelAdvanced, Binary: "pidstat", DefaultArgs: []string{"1"}},
	{Use: "perf", Short: "性能分析", Level: tools.LevelAdvanced, Binary: "perf", LinuxOnly: true, DefaultArgs: []string{"stat", "sleep", "3"}},
	{Use: "biotop", Short: "块设备 IO top", Level: tools.LevelAdvanced, Binary: "biotop", LinuxOnly: true},
	{Use: "blktrace", Short: "块 IO 追踪", Level: tools.LevelAdvanced, Binary: "blktrace", LinuxOnly: true},
	{Use: "tcptop", Short: "TCP 连接流量", Level: tools.LevelAdvanced, Binary: "tcptop", LinuxOnly: true},
	{Use: "tcpdump", Short: "抓包", Level: tools.LevelAdvanced, Binary: "tcpdump", DefaultArgs: []string{"-i", "any", "-c", "10"}},
}

var wrapCmd = &cobra.Command{
	Use:   "run",
	Short: "透传运行外部监控命令（中级/高级）",
	Long: `子命令对应常见系统工具，自动检测是否安装。
额外参数会追加到默认参数之后，例如:
  tipshow run iostat -- -d 1 5
  tipshow run tcpdump -- -i en0 -c 20`,
}

func init() {
	for _, def := range wrapDefs {
		def := def
		c := &cobra.Command{
			Use:   def.Use,
			Short: fmt.Sprintf("[%s] %s", def.Level, def.Short),
			Run: func(cmd *cobra.Command, args []string) {
				if def.LinuxOnly && !tools.Available(def.Binary) {
					// still check linux at runtime via platform in error message
				}
				bin := tools.Resolve(append([]string{def.Binary}, def.Alt...)...)
				if bin == "" {
					hint := "请运行 tipshow doctor 查看安装提示"
					fail(fmt.Errorf("%s 未安装 (%s)", def.Binary, hint))
				}
				argv := []string{def.Binary}
				argv = append(argv, def.DefaultArgs...)
				argv = append(argv, args...)

				if output.JSONMode {
					res, err := runner.Run(context.Background(), argv, 60*time.Second)
					if err != nil {
						fail(err)
					}
					_ = output.PrintJSON(map[string]any{
						"tool":   def.Binary,
						"level":  def.Level,
						"argv":   argv,
						"stdout": res.Stdout,
						"stderr": res.Stderr,
						"code":   res.ReturnCode,
					})
					return
				}

				// 交互式工具直接继承 stdio
				if def.Use == "htop" || def.Use == "iotop" {
					interactive := append([]string{def.Binary}, args...)
					if len(args) == 0 {
						interactive = argv
					}
					c := execCommand(interactive)
					if err := c.Run(); err != nil {
						fail(err)
					}
					return
				}

				if err := runner.RunOrPrint(context.Background(), argv, 120*time.Second); err != nil {
					fail(err)
				}
			},
		}
		wrapCmd.AddCommand(c)
	}
}
