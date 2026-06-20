package cmd

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/shengsheng/tipshow/internal/collect"
	"github.com/shengsheng/tipshow/internal/output"
	"github.com/shengsheng/tipshow/internal/runner"
	"github.com/shengsheng/tipshow/internal/tools"
	"github.com/spf13/cobra"
)

var netCmd = &cobra.Command{
	Use:   "net",
	Short: "网络：接口 IO / 连接视图",
}

var netIOCmd = &cobra.Command{
	Use:   "io",
	Short: "各网卡收发字节数（基础层，内置 gopsutil）",
	Run: func(cmd *cobra.Command, args []string) {
		counters, err := collect.NetIO()
		if err != nil {
			fail(err)
		}
		if err := output.Emit(counters, func() string {
			var b strings.Builder
			for _, c := range counters {
				if c.BytesSent == 0 && c.BytesRecv == 0 && strings.HasPrefix(c.Name, "lo") {
					continue
				}
				fmt.Fprintf(&b, "%-16s ↓ %-10s  ↑ %-10s\n",
					c.Name, collect.FormatBytes(c.BytesRecv), collect.FormatBytes(c.BytesSent))
			}
			return strings.TrimRight(b.String(), "\n")
		}); err != nil {
			fail(err)
		}
	},
}

var netConnCmd = &cobra.Command{
	Use:   "conn",
	Short: "网络连接（ss > netstat > lsof，中级层）",
	Run: func(cmd *cobra.Command, args []string) {
		var argv []string
		switch {
		case tools.Available("ss"):
			argv = []string{"ss", "-tunap"}
		case tools.Available("netstat"):
			argv = []string{"netstat", "-anv"}
		case tools.Available("lsof"):
			argv = []string{"lsof", "-i", "-P", "-n"}
		default:
			fail(fmt.Errorf("未找到 ss/netstat/lsof，请运行 tipshow doctor"))
		}
		if len(args) > 0 {
			argv = append(argv, args...)
		}
		if output.JSONMode {
			res, err := runner.Run(context.Background(), argv, 30*time.Second)
			if err != nil {
				fail(err)
			}
			_ = output.PrintJSON(map[string]any{"tool": argv[0], "stdout": res.Stdout, "stderr": res.Stderr})
			return
		}
		if err := runner.RunOrPrint(context.Background(), argv, 30*time.Second); err != nil {
			fail(err)
		}
	},
}

var netIfCmd = &cobra.Command{
	Use:   "if",
	Short: "网络接口（ip > ifconfig，中级层）",
	Run: func(cmd *cobra.Command, args []string) {
		var argv []string
		switch {
		case tools.Available("ip"):
			argv = []string{"ip", "addr"}
		case tools.Available("ifconfig"):
			argv = []string{"ifconfig"}
		default:
			fail(fmt.Errorf("未找到 ip/ifconfig"))
		}
		if len(args) > 0 {
			argv = append(argv, args...)
		}
		if output.JSONMode {
			res, err := runner.Run(context.Background(), argv, 15*time.Second)
			if err != nil {
				fail(err)
			}
			_ = output.PrintJSON(map[string]any{"tool": argv[0], "stdout": res.Stdout})
			return
		}
		if err := runner.RunOrPrint(context.Background(), argv, 15*time.Second); err != nil {
			fail(err)
		}
	},
}

func init() {
	netCmd.AddCommand(netIOCmd)
	netCmd.AddCommand(netConnCmd)
	netCmd.AddCommand(netIfCmd)
}
