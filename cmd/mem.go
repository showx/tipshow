package cmd

import (
	"fmt"
	"strings"

	"github.com/shengsheng/tipshow/internal/collect"
	"github.com/shengsheng/tipshow/internal/output"
	"github.com/spf13/cobra"
)

var memCmd = &cobra.Command{
	Use:   "mem",
	Short: "查看内存与 swap（基础层，内置 gopsutil）",
	Run: func(cmd *cobra.Command, args []string) {
		info, err := collect.Memory()
		if err != nil {
			fail(err)
		}
		if err := output.Emit(info, func() string {
			var b strings.Builder
			fmt.Fprintf(&b, "内存: %s / %s (%.1f%% 已用)\n",
				collect.FormatBytes(info.Used), collect.FormatBytes(info.Total), info.UsedPercent)
			fmt.Fprintf(&b, "可用: %s\n", collect.FormatBytes(info.Available))
			if info.SwapTotal > 0 {
				fmt.Fprintf(&b, "Swap: %s / %s\n",
					collect.FormatBytes(info.SwapUsed), collect.FormatBytes(info.SwapTotal))
			}
			return strings.TrimRight(b.String(), "\n")
		}); err != nil {
			fail(err)
		}
	},
}
