declare module "react-window" {
  import * as React from "react";
  import type { ComponentType, CSSProperties } from "react";

  export type GridChildComponentProps<T = unknown> = {
    columnIndex: number;
    rowIndex: number;
    style: CSSProperties;
    data: T;
    isScrolling?: boolean;
  };

  export type FixedSizeGridProps<T = unknown> = {
    children: ComponentType<GridChildComponentProps<T>>;
    className?: string;
    columnCount: number;
    columnWidth: number;
    height: number;
    itemData?: T;
    overscanColumnCount?: number;
    overscanRowCount?: number;
    rowCount: number;
    rowHeight: number;
    width: number;
  };

  export type ListChildComponentProps<T = unknown> = {
    index: number;
    style: CSSProperties;
    data: T;
    isScrolling?: boolean;
  };

  export type FixedSizeListProps<T = unknown> = {
    children: ComponentType<ListChildComponentProps<T>>;
    className?: string;
    height: number;
    itemCount: number;
    itemData?: T;
    itemSize: number;
    overscanCount?: number;
    width: number | string;
  };

  export class FixedSizeList<T = unknown> extends React.Component<FixedSizeListProps<T>> {
    scrollToItem(index: number, align?: "auto" | "smart" | "center" | "end" | "start"): void;
  }

  export class FixedSizeGrid<T = unknown> extends React.Component<
    FixedSizeGridProps<T>
  > {
    scrollTo(params: { scrollLeft?: number; scrollTop?: number }): void;
    scrollToItem(params: {
      align?: "auto" | "smart" | "center" | "end" | "start";
      columnIndex?: number;
      rowIndex?: number;
    }): void;
  }
}
