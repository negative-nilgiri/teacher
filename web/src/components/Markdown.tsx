import ReactMarkdown from "react-markdown";
import rehypeKatex from "rehype-katex";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import { ReferenceLink } from "./ReferenceLink";

interface MarkdownProps {
  children: string;
  className?: string;
}

export function Markdown({ children, className }: MarkdownProps) {
  return (
    <div className={className ? `markdown ${className}` : "markdown"}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[[rehypeKatex, { trust: false }]]}
        components={{
          a: ({ node: _node, href, children, ...props }) =>
            href?.startsWith("#") ? (
              <ReferenceLink destination={href.slice(1)}>{children}</ReferenceLink>
            ) : (
              <a {...props} href={href} rel="noreferrer" target="_blank">
                {children}
              </a>
            ),
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
