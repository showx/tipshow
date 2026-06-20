package runner

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"os/exec"
	"time"
)

type Result struct {
	Argv       []string
	ReturnCode int
	Stdout     string
	Stderr     string
}

func (r Result) OK() bool {
	return r.ReturnCode == 0
}

func Run(ctx context.Context, argv []string, timeout time.Duration) (Result, error) {
	if len(argv) == 0 {
		return Result{}, fmt.Errorf("empty argv")
	}
	if timeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, timeout)
		defer cancel()
	}

	cmd := exec.CommandContext(ctx, argv[0], argv[1:]...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err := cmd.Run()
	rc := 0
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			rc = exitErr.ExitCode()
		} else {
			return Result{}, err
		}
	}

	return Result{
		Argv:       argv,
		ReturnCode: rc,
		Stdout:     stdout.String(),
		Stderr:     stderr.String(),
	}, nil
}

func RunOrPrint(ctx context.Context, argv []string, timeout time.Duration) error {
	res, err := Run(ctx, argv, timeout)
	if err != nil {
		return err
	}
	if res.Stdout != "" {
		fmt.Print(res.Stdout)
	}
	if !res.OK() {
		msg := res.Stderr
		if msg == "" {
			msg = res.Stdout
		}
		if msg == "" {
			msg = fmt.Sprintf("exit %d", res.ReturnCode)
		}
		return fmt.Errorf("%s: %s", argv[0], msg)
	}
	if res.Stderr != "" {
		fmt.Fprint(os.Stderr, res.Stderr)
	}
	return nil
}
