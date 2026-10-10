<!-- Release Pipeline icon - 32x32
     为 32px 重新设计（非等比缩放）：
       - 丢弃阶段 1：最低透明度的那块在 32px 下只是噪点
       - 柱子压扁压矮，为箭头让出空间
       - 箭头加粗到 3.2px 并把箭杆加长，使其明显伸出箭翼之外

     构造法与主图一致。尖端 T=(25, 7.5)，描边 S=3.2，翼长 Lw=10，杆长 Ls=13。
     箭杆定位公式（绕自身中心顺时针旋转 45°，上端中心恰好落在 T）：
         left = T.x - 0.3536*Ls - S/2
         top  = T.y + 0.3536*Ls - Ls/2
     竖翼顶部比尖端再上探 0.7px，用于闭合与横翼的接缝。 -->
<Snapshot type="png" background="transparent">
    <Container
        width="32" height="32"
        borderRadius="7"
        gradientType="LINEAR"
        gradientColors="#0B1B4D,#1E3A8A,#5B21B6"
        gradientStops="0,0.55,1"
        gradientBegin="TOP_LEFT"
        gradientEnd="BOTTOM_RIGHT">

        <Stack>

            <!-- 阶段 2：进行中 -->
            <Positioned left="5" bottom="4.5" width="4.5" height="6.5">
                <Container color="#22D3EEE0" borderRadius="1.5"/>
            </Positioned>

            <!-- 阶段 3：发布中 -->
            <Positioned left="11" bottom="4.5" width="4.5" height="10">
                <Container color="#67E8F9" borderRadius="1.5"/>
            </Positioned>

            <!-- 箭头横翼：从尖端向左 -->
            <Positioned left="15" top="5.9" width="10" height="3.2">
                <Container color="#F0F9FF" borderRadius="1.6"/>
            </Positioned>

            <!-- 箭头竖翼：从尖端向下，顶部多探出 0.7px 闭合接缝 -->
            <Positioned left="23.4" top="6.8" width="3.2" height="10.7">
                <Container color="#F0F9FF" borderRadius="1.6"/>
            </Positioned>

            <!-- 箭杆：旋转 45°，上端落在尖端 -->
            <Positioned left="18.8" top="5.6" width="3.2" height="13">
                <Transform
                    matrix="(0.7071,0.7071,0,0,-0.7071,0.7071,0,0,0,0,1,0,0,0,0,1)"
                    alignment="CENTER">
                    <Container width="3.2" height="13" color="#F0F9FF" borderRadius="1.6"/>
                </Transform>
            </Positioned>

        </Stack>
    </Container>
</Snapshot>