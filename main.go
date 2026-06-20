package main

import (
	"os"

	"github.com/shengsheng/tipshow/cmd"
)

func main() {
	if err := cmd.Execute(); err != nil {
		os.Exit(1)
	}
}
