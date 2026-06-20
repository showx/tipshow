package cmd

import (
	"fmt"
	"os"

	"github.com/shengsheng/tipshow/internal/output"
	"github.com/spf13/cobra"
)

var (
	jsonFlag bool
)

var rootCmd = &cobra.Command{
	Use:   "tipshow",
	Short: "系统监控 CLI 工具包（由浅入深）",
	Long: `tipshow 统一封装常见 Linux/macOS 监控命令，便于统计与分析。

层级说明:
  basic         内置采集 (CPU/内存/磁盘/主机) + df/du
  intermediate  htop/iostat/iotop/netstat/ss/ip/ifconfig/lsof
  advanced      sar/pidstat/perf/iotop/blktrace/tcptop/tcpdump

先用 tipshow doctor 查看本机可用工具。`,
	PersistentPreRun: func(cmd *cobra.Command, args []string) {
		output.JSONMode = jsonFlag
	},
}

func Execute() error {
	return rootCmd.Execute()
}

func init() {
	rootCmd.PersistentFlags().BoolVar(&jsonFlag, "json", false, "以 JSON 输出（便于脚本分析）")
	rootCmd.AddCommand(doctorCmd)
	rootCmd.AddCommand(cpuCmd)
	rootCmd.AddCommand(memCmd)
	rootCmd.AddCommand(hostCmd)
	rootCmd.AddCommand(diskCmd)
	rootCmd.AddCommand(netCmd)
	rootCmd.AddCommand(wrapCmd)
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, "错误:", err)
	os.Exit(1)
}
