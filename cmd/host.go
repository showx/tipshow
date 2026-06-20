package cmd

import (
	"fmt"
	"strings"
	"time"

	"github.com/shengsheng/tipshow/internal/collect"
	"github.com/shengsheng/tipshow/internal/output"
	"github.com/spf13/cobra"
)

var hostCmd = &cobra.Command{
	Use:   "host",
	Short: "主机概览：系统版本、运行时间、负载（基础层）",
	Run: func(cmd *cobra.Command, args []string) {
		info, err := collect.Host()
		if err != nil {
			fail(err)
		}
		if err := output.Emit(info, func() string {
			uptime := time.Duration(info.Uptime) * time.Second
			var b strings.Builder
			fmt.Fprintf(&b, "主机: %s\n", info.Hostname)
			fmt.Fprintf(&b, "系统: %s %s (%s)\n", info.Platform, info.PlatformVersion, info.OS)
			fmt.Fprintf(&b, "内核: %s\n", info.Kernel)
			fmt.Fprintf(&b, "运行: %s (自 %s)\n", uptime.Truncate(time.Second), info.BootTime.Format(time.RFC3339))
			fmt.Fprintf(&b, "负载: %.2f / %.2f / %.2f\n", info.Load1, info.Load5, info.Load15)
			return strings.TrimRight(b.String(), "\n")
		}); err != nil {
			fail(err)
		}
	},
}
